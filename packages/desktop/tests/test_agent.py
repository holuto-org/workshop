"""Receipt execution and durable recovery against the socket simulator."""

import copy
import json
import unicodedata
from dataclasses import replace
from datetime import datetime

import pytest
from fiscal_desktop.agent import Agent, Api, validate_result
from posnet.printer import Printer
from posnet.protocol import ProtocolError
from posnet.simulator import Simulator

JOB_ID = "00000000-0000-0000-0000-000000000007"
ATTEMPT = "00000000-0000-0000-0000-000000000008"
PRINTER_ID = "00000000-0000-0000-0000-000000000009"
JOB = {
    "id": JOB_ID,
    "orderId": "00000000-0000-0000-0000-000000000010",
    "printerId": PRINTER_ID,
    "state": "taken",
    "kind": "sale",
    "allocationId": None,
    "advances": [],
    "payments": [{"form": "card", "amount": "250.00"}],
    "orderNumber": 10,
    "companyName": "Warsztat Kowalski",
    "buyerNip": None,
    "total": "250.00",
    "lines": [
        {
            "name": "Wymiana oleju",
            "quantity": "2",
            "unitPrice": "100.00",
            "total": "200.00",
            "vatRate": "23",
            "vatSlot": 0,
        },
        {
            "name": "Klocki hamulcowe",
            "quantity": "1",
            "unitPrice": "50.00",
            "total": "50.00",
            "vatRate": "8",
            "vatSlot": 1,
        },
    ],
    "attempt": ATTEMPT,
    "receiptNumber": None,
    "result": None,
    "createdAt": "2026-10-16T10:00:00Z",
    "takenAt": "2026-10-16T10:01:00Z",
    "resolvedAt": None,
    "etag": "0123456789abcdef0123456789abcdef",
}
ADVANCE_JOB = {
    **JOB,
    "kind": "advance",
    "allocationId": "00000000-0000-0000-0000-000000000012",
    "payments": [{"form": "mobile", "amount": "250.00"}],
    "lines": [
        {
            "name": "Zaliczka – naprawa pojazdu, zlecenie 10",
            "quantity": "1",
            "unitPrice": "200.00",
            "total": "200.00",
            "vatRate": "23",
            "vatSlot": 0,
        },
        {
            "name": "Zaliczka – naprawa pojazdu, zlecenie 10",
            "quantity": "1",
            "unitPrice": "50.00",
            "total": "50.00",
            "vatRate": "8",
            "vatSlot": 1,
        },
    ],
}
PRINTER = {
    "id": PRINTER_ID,
    "branchId": "00000000-0000-0000-0000-000000000011",
    "name": "Workshop",
    "deviceSerial": "DEMO0000001",
    "model": "POSNET TEMO ONLINE",
    "firmware": "32.01",
    "appVersion": "0.1.2",
    "vatSlots": [],
    "maxItemNameLength": 80,
    "maxLinesPerReceipt": 500,
    "allowedCharacters": "ABCabc 0123456789.,-ĄąĘęŁłŃńÓóŚśŹźŻżĆć",
}
STATUS = {
    "printer": PRINTER,
    "isOnline": True,
    "lastSeenAt": "2026-10-16T10:00:00Z",
    "updatedAt": "2026-10-16T09:00:00Z",
}


class FakeApi(Api):
    def __init__(self, jobs=()):
        super().__init__("https://api.example", "token", "DEMO0000001")
        self.printer_id = PRINTER_ID
        self.jobs = copy.deepcopy(list(jobs))
        self.remote = {JOB_ID: copy.deepcopy(JOB)}
        self.reports = []
        self.heartbeats = []
        self.fail_report = False

    def heartbeat(self):
        self.heartbeats.append(copy.deepcopy(self.telemetry))
        return STATUS

    def take(self):
        return self.jobs.pop(0) if self.jobs else None

    def status(self, job_id):
        return copy.deepcopy(self.remote[job_id])

    def report(self, job_id, body):
        validate_result(body)
        assert body["state"] != "done" or body["receiptNumber"]
        self.reports.append(copy.deepcopy(body))
        if self.fail_report:
            raise OSError("response lost")
        self.remote[job_id].update(body)
        return copy.deepcopy(self.remote[job_id])


def test_prints_once_with_brand_and_receipt_number(tmp_path):
    with Simulator() as sim:
        sim.header = "&c&1Firma Inna&1&c\nul. Testowa 1"
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([JOB])
        Agent(printer, api).step()
        assert len(sim.receipts) == 1
        assert sim.receipts[0]["total_cents"] == 25000
        assert sim.receipts[0]["payments"] == [{"ty": "2", "wa": "25000"}]
        assert [line["vt"] for line in sim.receipts[0]["lines"]] == ["0", "1"]
        assert sim.footers == [{"id": "25", "na": "Warsztat Kowalski"}]
        assert api.reports[0] == {
            "attempt": ATTEMPT,
            "state": "done",
            "receiptNumber": "1",
            "issuedAt": api.reports[0]["issuedAt"],
            "result": None,
        }
        assert datetime.fromisoformat(api.reports[0]["issuedAt"]).tzinfo is not None
        record = printer.last_record()
        assert record["state"] == "acknowledged"
        assert record["attempt"] == ATTEMPT
        assert record["job_id"] == JOB_ID
        assert len(api.heartbeats) == 1


@pytest.mark.parametrize("state", ["taken", "unknown"])
def test_report_retry_and_restart_preserve_exact_original_payload(tmp_path, state):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        api.fail_report = True
        with pytest.raises(OSError, match="response lost"):
            Agent(printer, api).step()
        original = copy.deepcopy(api.reports[0])
        assert json.loads(path.with_suffix(".agent.json").read_text())["body"] == original
        api.remote[JOB_ID]["state"] = state
        api.fail_report = False
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports == [original, original]
        assert len(sim.receipts) == 1
        assert printer.last_record()["state"] == "acknowledged"


def test_crash_after_device_completion_before_result_save_uses_durable_issue_time(
    tmp_path, monkeypatch
):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        agent = Agent(printer, api)
        monkeypatch.setattr(
            agent, "_report", lambda *_args, **_kwargs: (_ for _ in ()).throw(SystemExit)
        )
        with pytest.raises(SystemExit):
            agent.step()
        saved = printer.last_record()
        assert saved["state"] == "completed"
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports[0]["issuedAt"] == saved["issued_at"]
        assert api.reports[0]["attempt"] == ATTEMPT
        assert len(sim.receipts) == 1


def test_crash_before_device_journal_never_replays(tmp_path, monkeypatch):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        monkeypatch.setattr(
            printer, "print_receipt", lambda *_args, **_kwargs: (_ for _ in ()).throw(SystemExit)
        )
        with pytest.raises(SystemExit):
            Agent(printer, api).step()
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports[0]["state"] == "unknown"
        assert sim.receipts == []


def test_unknown_payload_is_stable_and_never_automatically_failed(tmp_path, monkeypatch):
    with Simulator() as sim:
        handle = sim._handle

        def dying(command, params):
            if command == "trend":
                raise ValueError
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", dying)
        path = tmp_path / "operation.json"
        api = FakeApi([JOB, JOB])
        api.fail_report = True
        with pytest.raises(OSError):
            Agent(Printer(sim.connection, path), api).step()
        original = copy.deepcopy(api.reports[0])
        assert original["state"] == "unknown"
        assert original["issuedAt"] is None
        api.fail_report = False
        agent = Agent(Printer(sim.connection, path), api)
        agent.step()
        agent.step()
        assert api.reports == [original, original]
        assert len(api.jobs) == 1
        assert len(api.heartbeats) == 3
        assert sim.receipts == []


def test_staff_resolution_unblocks_pending_without_printing_again(tmp_path, monkeypatch):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        original = printer.print_receipt

        def interrupted(*args, **kwargs):
            record = original(*args, **kwargs)
            record["state"] = "pending"
            printer._save(record)
            raise ProtocolError("interrupted")

        monkeypatch.setattr(printer, "print_receipt", interrupted)
        Agent(printer, api).step()
        api.remote[JOB_ID]["state"] = "failed"
        Agent(Printer(sim.connection, path), api).step()
        assert printer.pending() is None
        assert len(sim.receipts) == 1


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("state", "done"),
        ("attempt", None),
        ("id", 7),
        ("payments", 2),
        ("kind", "refund"),
        ("advances", None),
        ("printerId", JOB_ID),
        ("lines", None),
    ],
)
def test_malformed_or_unowned_response_never_reaches_physical_execution(tmp_path, field, value):
    with Simulator() as sim:
        bad = {**JOB, field: value}
        printer = Printer(sim.connection, tmp_path / "operation.json")
        with pytest.raises(ValueError):
            Agent(printer, FakeApi([bad])).step()
        assert sim.receipts == []
        assert printer.last_record() is None


def test_invalid_amount_fails_without_printing_and_retries_same_result(tmp_path):
    with Simulator() as sim:
        bad = copy.deepcopy(JOB)
        bad["lines"][0]["quantity"] = "NaN"
        path = tmp_path / "operation.json"
        api = FakeApi([bad])
        api.fail_report = True
        with pytest.raises(OSError):
            Agent(Printer(sim.connection, path), api).step()
        api.fail_report = False
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports[0]["state"] == "failed"
        assert api.reports[0] == api.reports[1]
        assert sim.receipts == []


def test_unready_printer_sends_fresh_telemetry_without_claiming(tmp_path, monkeypatch):
    with Simulator() as sim:
        handle = sim._handle
        monkeypatch.setattr(
            sim, "_handle", lambda c, p: {"pr": "5"} if c == "sprn" else handle(c, p)
        )
        api = FakeApi([JOB])
        Agent(Printer(sim.connection, tmp_path / "operation.json"), api).step()
        assert len(api.jobs) == 1
        assert api.heartbeats[0]["model"] == "POSNET TEMO ONLINE"
        assert sim.receipts == []


def test_probe_failure_still_sends_latest_heartbeat(tmp_path, monkeypatch):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([JOB])
        api.describe(printer.probe())
        monkeypatch.setattr(
            printer, "probe", lambda: (_ for _ in ()).throw(ProtocolError("offline"))
        )
        with pytest.raises(ProtocolError, match="offline"):
            Agent(printer, api).step()
        assert api.heartbeats[0]["firmware"] == "32.01"
        assert len(api.jobs) == 1


def test_device_swap_is_rejected_before_claim(tmp_path, monkeypatch):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        status = printer.probe()
        monkeypatch.setattr(printer, "probe", lambda: replace(status, unique_number="OTHER"))
        api = FakeApi([JOB])
        with pytest.raises(ValueError, match="drukark"):
            Agent(printer, api).step()
        assert len(api.jobs) == 1
        assert sim.receipts == []


def test_telemetry_has_raw_serial_and_json_unicode(tmp_path):
    with Simulator() as sim:
        status = Printer(sim.connection, tmp_path / "operation.json").probe()
    api = Api("https://api.example", "token", "DEMO0000001")
    api.describe(replace(status, name="Żółw", version="wersja ą"))
    assert api.telemetry["model"] == "Żółw"
    assert api.telemetry["firmware"] == "wersja ą"
    assert api.telemetry["serial"] == "DEMO0000001"
    assert api.telemetry["appVersion"]
    assert api.telemetry["vatSlots"] == [
        {"slot": 0, "vatRate": "23"},
        {"slot": 1, "vatRate": "8"},
        {"slot": 2, "vatRate": "0"},
        {"slot": 6, "vatRate": "zw"},
    ]


# `Telemetry` in the backend's openapi/agent.json: all required, no other properties.
TELEMETRY_FIELDS = {
    "maxItemNameLength",
    "maxLinesPerReceipt",
    "allowedCharacters",
    "serial",
    "model",
    "firmware",
    "appVersion",
    "vatSlots",
}


def test_described_telemetry_satisfies_the_agent_contract(tmp_path):
    with Simulator() as sim:
        status = Printer(sim.connection, tmp_path / "operation.json").probe()
    api = Api("https://api.example", "token", "DEMO0000001")
    api.describe(status)
    telemetry = json.loads(json.dumps(api.telemetry))  # exactly what goes on the wire
    assert set(telemetry) == TELEMETRY_FIELDS
    for field in ("serial", "model", "firmware", "appVersion"):  # Code: 1-64 printable chars
        assert 1 <= len(telemetry[field]) <= 64 and telemetry[field].isprintable()
    # PrinterLimit: a positive int32; the driver's documented Temo Online limits.
    assert telemetry["maxItemNameLength"] == 80
    assert telemetry["maxLinesPerReceipt"] == 500
    # AllowedCharacters: NFC, no C0/C1 controls, and exactly what the driver prints unchanged.
    characters = telemetry["allowedCharacters"]
    assert characters == unicodedata.normalize("NFC", characters)
    assert not any(unicodedata.category(c) == "Cc" for c in characters)
    assert len(set(characters)) == len(characters) == 216
    unassigned = (0x81, 0x83, 0x88, 0x90, 0x98)  # no Unicode character in CP1250
    unprintable = (0x7F, 0xA0, 0xAD)  # DEL, no-break space, soft hyphen
    assert characters.encode("cp1250") == bytes(
        code for code in range(0x20, 0x100) if code not in unassigned + unprintable
    )
    assert {" ", "&", "€", "Ą", "ż", "ó", "ß"} <= set(characters)
    assert "\t" not in characters and "Ж" not in characters


def test_exempt_workshop_uses_exempt_slot(tmp_path, monkeypatch):
    with Simulator() as sim:
        handle = sim._handle
        rates = dict.fromkeys((f"v{letter}" for letter in "abcdef"), "101,00") | {"vg": "100,00"}
        monkeypatch.setattr(sim, "_handle", lambda c, p: rates if c == "vatget" else handle(c, p))
        printer = Printer(sim.connection, tmp_path / "operation.json")
        job = copy.deepcopy(JOB)
        for line in job["lines"]:
            line.update(vatRate="zw", vatSlot=6)
        api = FakeApi([job])
        Agent(printer, api).step()
        assert api.reports[0]["state"] == "done"
        assert {line["vt"] for line in sim.receipts[0]["lines"]} == {"6"}


def test_corrupt_agent_journal_blocks_printing(tmp_path):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        agent = Agent(Printer(sim.connection, path), FakeApi([JOB]))
        agent.outbox_path.write_text("not json")
        with pytest.raises(ValueError):
            agent.step()
        assert sim.receipts == []


def test_orphan_device_journal_without_attempt_fails_closed(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        printer._save({"state": "completed", "job_id": JOB_ID})
        with pytest.raises(ValueError):
            Agent(printer, FakeApi([JOB])).step()
        assert sim.receipts == []


@pytest.mark.parametrize("result_source", ["dispatch", "crash", "rejected"])
def test_missing_receipt_number_stays_unknown_until_staff_resolution(
    tmp_path, monkeypatch, result_source
):
    with Simulator() as sim:
        handle = sim._handle

        def counter_failure(command, params):
            if command == "scnt":
                raise ValueError
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", counter_failure)
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        agent = Agent(printer, api)
        if result_source == "crash":
            monkeypatch.setattr(
                agent, "_report", lambda *_args, **_kwargs: (_ for _ in ()).throw(SystemExit)
            )
            with pytest.raises(SystemExit):
                agent.step()
        else:
            agent.step()
        if result_source == "rejected":
            saved = json.loads(agent.outbox_path.read_text())
            saved["body"].update(state="done", issuedAt=saved["issuedAt"])
            agent.outbox_path.write_text(json.dumps(saved))
            api.remote[JOB_ID]["state"] = "taken"
            api.reports.clear()
        agent = Agent(Printer(sim.connection, path), api)
        agent.step()
        agent.step()
        assert len(api.reports) == 1
        assert api.reports[0]["state"] == "unknown"
        assert api.reports[0]["receiptNumber"] is None
        assert api.reports[0]["issuedAt"] is None
        assert "numer" in api.reports[0]["result"]
        assert printer.last_record()["state"] == "completed"
        assert agent.blocked
        assert len(sim.receipts) == 1
        api.remote[JOB_ID].update(state="done", receiptNumber="staff-number")
        agent.step()
        assert not agent.blocked
        assert printer.last_record()["state"] == "acknowledged"
        assert len(sim.receipts) == 1


def test_completed_evidence_upgrades_unknown_to_done_without_physical_replay(tmp_path, monkeypatch):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        original = printer.print_receipt

        def interrupted_after_completion(*args, **kwargs):
            original(*args, **kwargs)
            raise ProtocolError("lost final response")

        monkeypatch.setattr(printer, "print_receipt", interrupted_after_completion)
        Agent(printer, api).step()
        assert api.reports[0]["state"] == "unknown"
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports[1]["state"] == "done"
        assert api.reports[1]["issuedAt"] == printer.last_record()["issued_at"]
        assert len(sim.receipts) == 1


def test_remote_attempt_mismatch_preserves_journals_and_blocks(tmp_path):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        api.fail_report = True
        with pytest.raises(OSError):
            Agent(printer, api).step()
        api.fail_report = False
        api.remote[JOB_ID]["attempt"] = PRINTER_ID
        with pytest.raises(ValueError):
            Agent(Printer(sim.connection, path), api).step()
        assert printer.last_record()["state"] == "completed"
        assert len(api.reports) == 1
        assert len(sim.receipts) == 1


def test_accepted_result_followed_by_ack_failure_settles_without_reporting_again(
    tmp_path, monkeypatch
):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])

        def acknowledgement_failure(_record):
            raise OSError("disk full")

        monkeypatch.setattr(printer, "acknowledge", acknowledgement_failure)
        with pytest.raises(OSError, match="disk full"):
            Agent(printer, api).step()
        Agent(Printer(sim.connection, path), api).step()
        assert len(api.reports) == 1
        assert json.loads(path.with_suffix(".agent.json").read_text())["body"] == api.reports[0]
        assert printer.last_record()["state"] == "acknowledged"
        assert len(sim.receipts) == 1


def test_repeated_taken_response_for_same_job_is_not_reprinted(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([JOB, JOB])
        agent = Agent(printer, api)
        agent.step()
        with pytest.raises(ValueError):
            agent.step()
        assert len(sim.receipts) == 1


def test_second_probe_rejects_device_swap_after_claim(tmp_path, monkeypatch):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        status = printer.probe()
        probes = iter((status, replace(status, unique_number="OTHER")))
        monkeypatch.setattr(printer, "probe", lambda: next(probes))
        api = FakeApi([JOB])
        with pytest.raises(ValueError, match="drukark"):
            Agent(printer, api).step()
        assert sim.receipts == []
        assert printer.last_record() is None


def test_error_text_is_bounded_once_before_persisting_result(tmp_path, monkeypatch):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)

        def invalid_receipt(*_args, **_kwargs):
            raise ValueError("x" * 5000)

        monkeypatch.setattr(printer, "print_receipt", invalid_receipt)
        api = FakeApi([JOB])
        api.fail_report = True
        with pytest.raises(OSError):
            Agent(printer, api).step()
        api.fail_report = False
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports[0] == api.reports[1]
        assert len(api.reports[0]["result"]) == 2000
        assert sim.receipts == []


def test_failed_report_is_not_retried_over_remote_unknown(tmp_path):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        bad = copy.deepcopy(JOB)
        bad["lines"][0]["quantity"] = "NaN"
        api = FakeApi([bad])
        api.fail_report = True
        with pytest.raises(OSError):
            Agent(Printer(sim.connection, path), api).step()
        api.fail_report = False
        api.remote[JOB_ID]["state"] = "unknown"
        Agent(Printer(sim.connection, path), api).step()
        assert len(api.reports) == 1
        assert api.reports[0]["state"] == "failed"
        assert not json.loads(path.with_suffix(".agent.json").read_text())["settled"]
        assert sim.receipts == []


def test_agent_lock_prevents_a_second_process_from_dispatching(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        first = Agent(printer, FakeApi([JOB]))
        second = Agent(printer, FakeApi([JOB]))
        with first._locked(), pytest.raises(ValueError, match="Inny agent"):
            second.step()
        assert sim.receipts == []


def test_outbox_write_failure_happens_before_physical_dispatch(tmp_path, monkeypatch):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")

        def disk_failure(_path, _record):
            raise OSError("disk full")

        monkeypatch.setattr("fiscal_desktop.agent.durable_save", disk_failure)
        with pytest.raises(OSError, match="disk full"):
            Agent(printer, FakeApi([JOB])).step()
        assert printer.last_record() is None
        assert sim.receipts == []


@pytest.mark.parametrize("terminal", ["done", "failed", "expired"])
def test_terminal_remote_result_acknowledges_completed_journal_without_overwriting(
    tmp_path, terminal
):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        api.fail_report = True
        with pytest.raises(OSError):
            Agent(printer, api).step()
        original = copy.deepcopy(api.reports[0])
        api.fail_report = False
        api.remote[JOB_ID].update(
            state=terminal,
            receiptNumber="staff-evidence" if terminal == "done" else None,
            result="Resolved by staff",
        )
        Agent(Printer(sim.connection, path), api).step()
        Agent(Printer(sim.connection, path), api).step()
        assert api.reports == [original]
        assert api.remote[JOB_ID]["result"] == "Resolved by staff"
        assert printer.last_record()["state"] == "acknowledged"
        assert len(sim.receipts) == 1


@pytest.mark.parametrize("completed", [True, False])
def test_terminal_resolution_before_first_result_remains_settled_after_restart(
    tmp_path, monkeypatch, completed
):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        agent = Agent(printer, api)

        def process_death(*_args, **_kwargs):
            raise SystemExit

        if completed:
            monkeypatch.setattr(agent, "_report", process_death)
        else:
            monkeypatch.setattr(printer, "print_receipt", process_death)
        with pytest.raises(SystemExit):
            agent.step()
        api.remote[JOB_ID].update(
            state="done" if completed else "failed", result="Staff resolution"
        )
        Agent(Printer(sim.connection, path), api).step()
        Agent(Printer(sim.connection, path), api).step()
        saved = json.loads(path.with_suffix(".agent.json").read_text())
        assert saved["settled"]
        assert saved["body"] is None
        assert saved["resolvedState"] == ("done" if completed else "failed")
        assert api.reports == []
        assert len(sim.receipts) == int(completed)


def test_unmappable_active_vat_slot_is_omitted_and_logged(tmp_path, caplog):
    from decimal import Decimal

    from posnet.models import VatRate

    with Simulator() as sim:
        status = Printer(sim.connection, tmp_path / "operation.json").probe()
    api = FakeApi()
    api.describe(replace(status, vat_rates=[VatRate(0, Decimal(22)), VatRate(1, Decimal(5))]))
    assert api.telemetry["vatSlots"] == [{"slot": 1, "vatRate": "5"}]
    assert "slocie 0" in caplog.text


def test_job_rate_must_match_the_reported_printer_slot(tmp_path):
    with Simulator() as sim:
        job = copy.deepcopy(JOB)
        job["lines"][0]["vatSlot"] = 1
        api = FakeApi([job])
        Agent(Printer(sim.connection, tmp_path / "operation.json"), api).step()
        assert api.reports[0]["state"] == "failed"
        assert sim.receipts == []


def test_unknown_receipt_blocks_manual_reports_and_cannot_be_acknowledged_locally(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        printer._save({"state": "pending", "operation": "receipt"})
        agent = Agent(printer, FakeApi())
        with pytest.raises(ValueError):
            agent.run_report("daily_report")
        with pytest.raises(ValueError, match="Holuto"):
            agent.run_report("acknowledge_pending")
        assert sim.reports == []
        assert printer.pending() is not None


def test_advance_job_prints_one_line_per_vat_rate(tmp_path):
    with Simulator() as sim:
        api = FakeApi([ADVANCE_JOB])
        Agent(Printer(sim.connection, tmp_path / "operation.json"), api).step()
        assert api.reports[0]["state"] == "done"
        receipt = sim.receipts[0]
        assert receipt["total_cents"] == 25000
        assert [(line["vt"], line["il"], line["pr"]) for line in receipt["lines"]] == [
            ("0", "1", "20000"),
            ("1", "1", "5000"),
        ]
        assert receipt["advances"] == []
        assert receipt["payments"] == [{"ty": "6", "wa": "25000", "na": "Płatność mobilna"}]


@pytest.mark.parametrize(
    ("kind", "advances", "payments", "fiscal_cents"),
    [
        (
            "sale",
            [{"vatSlot": 0, "vatRate": "23", "amount": "100.00"}],
            [{"form": "card", "amount": "50.00"}, {"form": "credit", "amount": "100.00"}],
            15000,
        ),
        (
            "sale",
            [
                {"vatSlot": 0, "vatRate": "23", "amount": "200.00"},
                {"vatSlot": 1, "vatRate": "8", "amount": "50.00"},
            ],
            [],
            0,
        ),
    ],
)
def test_job_settlement_reaches_printer(tmp_path, kind, advances, payments, fiscal_cents):
    job = {**JOB, "kind": kind, "advances": advances, "payments": payments}
    with Simulator() as sim:
        api = FakeApi([job])
        Agent(Printer(sim.connection, tmp_path / "operation.json"), api).step()
        assert api.reports[0]["state"] == "done"
        assert sim.receipts[0]["total_cents"] == fiscal_cents
        assert len(sim.receipts[0]["advances"]) == len(advances)
        assert len(sim.receipts[0]["payments"]) == len(payments)


def test_duplicate_advance_slot_reports_clear_failure_before_printing(tmp_path):
    job = {
        **JOB,
        "advances": [
            {"vatSlot": 0, "vatRate": "23", "amount": "40.00"},
            {"vatSlot": 0, "vatRate": "23", "amount": "60.00"},
        ],
        "payments": [{"form": "card", "amount": "150.00"}],
    }
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([job])
        Agent(printer, api).step()
        assert api.reports[0]["state"] == "failed"
        assert "Powtórzony slot VAT zaliczki" in api.reports[0]["result"]
        assert not sim.transaction_open
        assert sim.receipts == []
        assert printer.last_record() is None


@pytest.mark.parametrize(
    "changes",
    [
        {"advances": [{"vatSlot": 0, "vatRate": "23", "amount": "200.01"}]},
        {
            "advances": [
                {"vatSlot": 0, "vatRate": "23", "amount": "100.00"},
                {"vatSlot": 0, "vatRate": "23", "amount": "100.01"},
            ]
        },
        {"payments": [{"form": "cash", "amount": "249.99"}]},
        {"payments": [{"form": "cash", "amount": "250.01"}]},
        {"advances": [{"vatSlot": 3, "vatRate": "23", "amount": "1.00"}]},
        {"advances": [{"vatSlot": 0, "vatRate": "8", "amount": "1.00"}]},
        {"payments": [{"form": "cash", "amount": "-1.00"}]},
        {"payments": [{"form": "cash", "amount": "NaN"}]},
        {"payments": [{"form": "cash", "amount": "Infinity"}]},
        {"payments": [{"form": "cash", "amount": "0.001"}]},
        {"payments": [{"form": "cash", "amount": "100000000.00"}]},
        {"advances": [{"vatSlot": 0, "vatRate": "23", "amount": "-1.00"}]},
        {"total": "249.99"},
        {"total": "100000000.00"},
    ],
)
def test_invalid_job_settlement_reports_failure_without_fiscal_commands(tmp_path, changes):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([{**JOB, **changes}])
        Agent(printer, api).step()
        assert api.reports[0]["state"] == "failed"
        assert not sim.transaction_open
        assert sim.receipts == []
        assert printer.last_record() is None


@pytest.mark.parametrize("history_error", ["truncated", "disk_full"])
def test_history_cache_failure_does_not_block_fiscal_settlement(
    tmp_path, monkeypatch, caplog, history_error
):
    with Simulator() as sim:
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = FakeApi([JOB])
        agent = Agent(printer, api)
        if history_error == "truncated":
            path.with_name("history.jsonl").write_text('{"job_id":')
        else:
            monkeypatch.setattr(
                agent,
                "_save_history",
                lambda *_: (_ for _ in ()).throw(OSError("disk full")),
            )
        agent.step()
        restarted = Agent(Printer(sim.connection, path), api)
        restarted.step()
        assert api.reports[0]["state"] == "done"
        assert len(api.reports) == 1
        assert printer.last_record()["state"] == "acknowledged"
        assert json.loads(agent.outbox_path.read_text())["settled"]
        assert not restarted.blocked
        assert len(sim.receipts) == 1
        assert "historii" in caplog.text


@pytest.mark.parametrize(
    ("changes", "reason"),
    [
        ({"unitPrice": "100.001"}, "Cena"),
        ({"total": "199.99"}, "Wartość pozycji"),
    ],
)
def test_invalid_job_line_fails_before_printing(tmp_path, changes, reason):
    job = copy.deepcopy(JOB)
    job["lines"][0].update(changes)
    if "total" in changes:
        job["lines"][1]["total"] = "50.01"
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([job])
        Agent(printer, api).step()
        assert api.reports[0]["state"] == "failed"
        assert reason in api.reports[0]["result"]
        assert printer.last_record() is None
        assert not sim.transaction_open
        assert sim.receipts == []


def test_job_lines_print_as_received_with_half_up_line_totals(tmp_path):
    job = copy.deepcopy(JOB)
    job["lines"][0].update(quantity="1.5", unitPrice="0.33", total="0.50")
    job["lines"][1].update(quantity="1", unitPrice="0.50", total="0.50")
    job.update(total="1.00", payments=[{"form": "card", "amount": "1.00"}])
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([job])
        Agent(printer, api).step()
        assert api.reports[0]["state"] == "done"
        assert [
            (line["quantity"], line["price_cents"], line["total_cents"])
            for line in printer.last_record()["lines"]
        ] == [("1.5", 33, 50), ("1", 50, 50)]
        assert sim.receipts[0]["total_cents"] == 100
