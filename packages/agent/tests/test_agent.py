"""Receipt execution and durable recovery against the socket simulator."""

import copy
import json
from dataclasses import replace
from datetime import datetime

import pytest
from posnet.printer import Printer
from posnet.protocol import ProtocolError
from posnet.simulator import Simulator
from workshop_agent.agent import Agent, Api

JOB_ID = "00000000-0000-0000-0000-000000000007"
ATTEMPT = "00000000-0000-0000-0000-000000000008"
PRINTER_ID = "00000000-0000-0000-0000-000000000009"
JOB = {
    "id": JOB_ID,
    "orderId": "00000000-0000-0000-0000-000000000010",
    "printerId": PRINTER_ID,
    "state": "taken",
    "payment": "card",
    "payload": {
        "companyName": "Warsztat Kowalski",
        "lines": [
            {"name": "Wymiana oleju", "quantity": "2", "unitPrice": "100.00"},
            {"name": "Klocki hamulcowe", "quantity": "1", "unitPrice": "50.00"},
        ],
    },
    "attempt": ATTEMPT,
    "receiptNumber": None,
    "result": None,
    "createdAt": "2026-10-16T10:00:00Z",
    "takenAt": "2026-10-16T10:01:00Z",
    "resolvedAt": None,
    "etag": "opaque-job-tag",
}
PRINTER = {
    "id": PRINTER_ID,
    "branchId": "00000000-0000-0000-0000-000000000011",
    "name": "Workshop",
    "deviceSerial": "DEMO0000001",
    "vendor": "posnet",
    "deviceModel": "POSNET TEMO ONLINE",
    "firmware": "32.01",
    "agentVersion": "0.1.2",
    "vatRates": [],
    "lastSeenAt": "2026-10-16T10:00:00Z",
    "isOnline": True,
    "etag": "opaque-printer-tag",
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
        return PRINTER

    def take(self):
        return self.jobs.pop(0) if self.jobs else None

    def status(self, job_id):
        return copy.deepcopy(self.remote[job_id])

    def report(self, job_id, body):
        self.reports.append(copy.deepcopy(body))
        if self.fail_report:
            raise OSError("response lost")
        self.remote[job_id].update(body)
        return copy.deepcopy(self.remote[job_id])


@pytest.fixture(autouse=True)
def no_wait(monkeypatch):
    monkeypatch.setattr("workshop_agent.agent.time.sleep", lambda _: None)


def test_prints_once_with_brand_and_receipt_number(tmp_path):
    with Simulator() as sim:
        sim.header = "&c&1Firma Inna&1&c\nul. Testowa 1"
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([JOB])
        Agent(printer, api).step()
        assert len(sim.receipts) == 1
        assert sim.receipts[0]["total_cents"] == 25000
        assert sim.receipts[0]["payment"] == 2
        assert {line["vt"] for line in sim.receipts[0]["lines"]} == {"0"}
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
        ("payment", 2),
        ("printerId", JOB_ID),
        ("payload", None),
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
        bad["payload"]["lines"][0]["quantity"] = "NaN"
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
    assert api.telemetry["agentVersion"] == "0.1.2"
    assert api.telemetry["vatRates"] == [
        {"index": 0, "percent": "23"},
        {"index": 1, "percent": "8"},
        {"index": 2, "percent": "0"},
        {"index": 6, "percent": "zw"},
    ]


def test_exempt_workshop_uses_exempt_slot(tmp_path, monkeypatch):
    with Simulator() as sim:
        handle = sim._handle
        rates = dict.fromkeys((f"v{letter}" for letter in "abcdef"), "101,00") | {"vg": "100,00"}
        monkeypatch.setattr(sim, "_handle", lambda c, p: rates if c == "vatget" else handle(c, p))
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = FakeApi([JOB])
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


def test_counter_failure_reports_done_with_null_number(tmp_path, monkeypatch):
    with Simulator() as sim:
        handle = sim._handle

        def counter_failure(command, params):
            if command == "scnt":
                raise ValueError
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", counter_failure)
        api = FakeApi([JOB])
        Agent(Printer(sim.connection, tmp_path / "operation.json"), api).step()
        assert api.reports[0]["state"] == "done"
        assert api.reports[0]["receiptNumber"] is None
        assert api.reports[0]["issuedAt"] is not None
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
        bad["payload"]["lines"][0]["quantity"] = "NaN"
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

        monkeypatch.setattr("workshop_agent.agent.durable_save", disk_failure)
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
