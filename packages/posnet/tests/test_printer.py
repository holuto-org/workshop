import os
from datetime import date, datetime
from decimal import Decimal

import pytest
from posnet.models import MAX_CENTS, Advance, Line, Payment
from posnet.printer import Printer, _try_lock, header_text, receipt_commands
from posnet.protocol import Connection, ProtocolError
from posnet.simulator import Simulator


def test_printer_lock_rejects_overlap_and_releases_after_connection_failure(tmp_path):
    printer = Printer(Connection(str(tmp_path / "missing-port")), tmp_path / "operation.json")
    lock_path = tmp_path / "operation.lock"
    with lock_path.open("ab") as lock:
        _try_lock(lock)  # another process is mid-operation
        with pytest.raises(ValueError):
            printer.probe()
    with pytest.raises(ProtocolError):
        printer.probe()
    with lock_path.open("ab") as lock:
        _try_lock(lock)  # the failed probe released its lock


def test_header_text_strips_formatting_and_keeps_literal_ampersand():
    assert header_text("&c&1Kowalski && Syn&1&c\nul. X") == "Kowalski & Syn\nul. X"


def test_brand_footer_only_when_header_lacks_company_name(tmp_path):
    lines = [Line("Wymiana oleju", Decimal(1), Decimal("100.00"), 0)]
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        record = printer.print_receipt(
            lines, [Payment("cash", 10000)], brand="warsztat kowalski", job_id=7
        )
        assert sim.footers == []
        assert record["receipt_number"] == "1"
        assert record["job_id"] == 7
        assert record["state"] == "completed"

        sim.header = "&cInny Serwis&c"
        printer.acknowledge(record)
        printer.print_receipt(
            lines, [Payment("card", 10000)], brand="Warsztat Kowalski", buyer_nip="5222628262"
        )
        assert sim.footers == [{"id": "25", "na": "Warsztat Kowalski"}]
        assert not sim.footer_open
        assert printer.last_record()["footer"] == "Warsztat Kowalski"
        assert sim.receipts[-1]["buyer_nip"] == "5222628262"


def test_detect_adopts_the_port_that_identifies_as_temo(tmp_path):
    with Simulator() as sim:
        printer = Printer(Connection("", 9600), tmp_path / "operation.json")
        status = printer.detect(["/dev/null", sim.connection.address])
        assert status.unique_number == "DEMO0000001"
        assert printer.connection.address == sim.connection.address
        assert printer.probe().ready  # the adopted port is now the saved one


def test_detect_reports_the_original_error_when_nothing_answers(tmp_path):
    printer = Printer(Connection("", 9600), tmp_path / "operation.json")
    with pytest.raises(ValueError, match="Nie wykryto drukarki"):
        printer.detect(["/dev/null"])
    assert printer.connection.address == ""


@pytest.mark.skipif(os.name == "nt" or os.geteuid() == 0, reason="needs a port root cannot open")
def test_detect_names_the_port_the_os_refused_to_open(tmp_path):
    port = tmp_path / "ttyACM0"
    port.touch(mode=0)
    printer = Printer(Connection("", 9600), tmp_path / "operation.json")
    with pytest.raises(ValueError, match=f"Brak uprawnień do portu {port}.*dialout"):
        printer.detect(["/dev/null", str(port)])
    assert printer.connection.address == ""


def test_detect_never_switches_device_while_an_operation_is_unresolved(tmp_path):
    with Simulator() as sim:
        printer = Printer(Connection("", 9600), tmp_path / "operation.json")
        (tmp_path / "operation.json").write_text(
            '{"state": "pending", "operation": "receipt", "unique_number": "OTHER"}',
            encoding="utf-8",
        )
        with pytest.raises(ValueError, match="Nie wykryto drukarki"):
            printer.detect([sim.connection.address])
        assert printer.connection.address == ""
        assert sim.receipts == []


@pytest.mark.parametrize(
    ("form", "wire_type"),
    [("cash", 0), ("card", 2), ("transfer", 8), ("credit", 5), ("mobile", 6)],
)
@pytest.mark.parametrize("buyer_nip", [None, "5222628262"])
def test_receipt_preserves_line_slots_buyer_nip_and_payments(tmp_path, form, wire_type, buyer_nip):
    lines = [
        Line("Usługa", Decimal(1), Decimal("10.00"), 0),
        Line("Część", Decimal(2), Decimal("2.50"), 1),
        Line("Sprzedaż zero", Decimal(1), Decimal("3.00"), 2),
        Line("Zwolniona", Decimal(1), Decimal("2.00"), 6),
    ]
    payments = [Payment(form, 2000)]
    commands = receipt_commands(lines, payments, [], "", buyer_nip)
    assert [(name, params) for name, params, _ in commands if name == "trnipset"] == (
        [] if buyer_nip is None else [("trnipset", {"ni": "5222628262"})]
    )
    if buyer_nip:
        assert commands[1] == ("trnipset", {"ni": "5222628262"}, 10)
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        record = printer.print_receipt(
            lines, payments, buyer_nip=buyer_nip, job_id="job-1", attempt="attempt-1"
        )
        assert sim.receipts == [
            {
                "total_cents": 2000,
                "payments": [
                    {"ty": str(wire_type), "wa": "2000"}
                    | ({"na": "Płatność mobilna"} if form == "mobile" else {})
                ],
                "advances": [],
                "buyer_nip": buyer_nip,
                "lines": [
                    {"na": "Usługa", "vt": "0", "pr": "1000", "il": "1", "wa": "1000"},
                    {"na": "Część", "vt": "1", "pr": "250", "il": "2", "wa": "500"},
                    {"na": "Sprzedaż zero", "vt": "2", "pr": "300", "il": "1", "wa": "300"},
                    {"na": "Zwolniona", "vt": "6", "pr": "200", "il": "1", "wa": "200"},
                ],
            }
        ]
        assert record["state"] == "completed"
        assert record["payments"] == [{"form": form, "amount_cents": 2000}]
        assert record["job_id"] == "job-1"
        assert record["attempt"] == "attempt-1"
        assert [line["vat"] for line in record["lines"]] == [0, 1, 2, 6]


@pytest.mark.parametrize("buyer_nip", ["", "   ", "1" * 31, "123\tni456", "123\n456", "🔧", 123])
def test_invalid_buyer_nip_is_rejected_before_opening_a_transaction(tmp_path, buyer_nip):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        with pytest.raises(ValueError, match="NIP"):
            printer.print_receipt(
                [Line("Olej", Decimal(1), Decimal("1.00"), 0)],
                [Payment("cash", 100)],
                buyer_nip=buyer_nip,
            )
        assert not sim.transaction_open
        assert sim.receipts == []
        assert printer.last_record() is None


def test_buyer_nip_does_not_leak_into_the_next_receipt(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        lines = [Line("Olej", Decimal(1), Decimal("1.00"), 0)]
        printer.print_receipt(lines, [Payment("cash", 100)], buyer_nip="5222628262")
        printer.print_receipt(lines, [Payment("cash", 100)])
        assert [receipt["buyer_nip"] for receipt in sim.receipts] == ["5222628262", None]


@pytest.mark.parametrize("failed_command", ["trnipset", "trpayment"])
def test_failure_mid_receipt_blocks_until_acknowledged_on_the_same_device(
    tmp_path, monkeypatch, failed_command
):
    lines = [Line("Wymiana oleju", Decimal(1), Decimal("100.00"), 0)]
    with Simulator() as sim:
        handle = sim._handle

        def dying(command, params):
            if command == failed_command:
                assert printer.pending()["stage"] == failed_command
                raise ValueError  # the device answers ?2000: outcome unknown to the driver
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", dying)
        printer = Printer(sim.connection, tmp_path / "operation.json")
        with pytest.raises(ProtocolError, match="Nie wysyłaj jej ponownie"):
            printer.print_receipt(lines, [Payment("cash", 10000)], buyer_nip="5222628262")
        assert sim.receipts == []
        assert printer.pending()["stage"] == failed_command
        printer = Printer(sim.connection, tmp_path / "operation.json")  # restart keeps the block

        monkeypatch.setattr(sim, "_handle", handle)
        with pytest.raises(ValueError, match="nierozstrzygnięta"):
            printer.print_receipt(lines, [Payment("cash", 10000)])
        assert sim.receipts == []

        with pytest.raises(ValueError, match="Otwarta transakcja"):
            printer.acknowledge_pending()  # the device itself is still mid-receipt
        sim.transaction_open = False  # cancelled on the printer's keypad
        printer._save({**printer.last_record(), "unique_number": "OTHER"})  # the printer died
        with pytest.raises(ValueError, match="Podłącz drukarkę"):
            printer.acknowledge_pending()
        printer.acknowledge_pending(force=True)
        assert printer.pending() is None
        assert printer.last_record()["resolution"] == "manual"


@pytest.mark.parametrize("content", ["", "{not json", '{"state": "odd"}', "[]"])
def test_unreadable_journal_blocks_every_operation(tmp_path, content):
    (tmp_path / "operation.json").write_text(content, encoding="utf-8")
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        with pytest.raises(ValueError, match="zablokowane"):
            printer.last_record()
        with pytest.raises(ValueError, match="zablokowane"):
            printer.print_receipt(
                [Line("Olej", Decimal(1), Decimal("1.00"), 0)], [Payment("cash", 100)]
            )
        assert sim.receipts == []


def test_reports_send_the_documented_commands_and_refuse_a_reversed_range(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        record = printer.daily_report()
        assert (record["operation"], record["state"]) == ("daily_report", "completed")
        printer.monthly_report(2026, 8)
        printer.periodic_report(date(2026, 8, 1), date(2026, 8, 31), summary=True)
        with pytest.raises(ValueError, match="późniejsza"):
            printer.periodic_report(date(2026, 8, 31), date(2026, 8, 1))
        assert sim.reports == [
            # The date is the printer's own, read from its clock, not the workshop PC's.
            {
                "command": "dailyrep",
                "params": {"da": datetime.now().astimezone().date().isoformat()},
            },
            {"command": "monthlyrep", "params": {"da": "2026-08-01", "su": "0"}},
            {
                "command": "periodicrepbydates",
                "params": {"fd": "2026-08-01", "td": "2026-08-31", "su": "1"},
            },
        ]


@pytest.mark.parametrize(
    ("command", "reply", "problem"),
    [
        ("getrealid", {"nm": "POSNET THERMAL HD", "vr": "1", "nu": "X"}, "nie jest drukarką Temo"),
        ("scomm", {"fs": "0", "hr": "1", "ts": "0"}, "trybie fiskalnym"),
        ("scomm", {"fs": "1", "hr": "0", "ts": "0"}, "zaprogramowanego nagłówka"),
        ("sdev", {"ds": "1", "qe": "1"}, "oczekuje na operatora"),
        ("sdev", {"ds": "0", "qe": "0"}, "oczekujące polecenia"),
        ("sprn", {"pr": "5"}, "Brak papieru"),
        ("strns", {"to": "1", "fe": "0"}, "Otwarta transakcja"),
        ("vatget", dict.fromkeys(("va", "vb", "vc", "vd", "ve", "vf", "vg"), "101,00"), "stawek"),
    ],
)
def test_preflight_refuses_every_documented_device_problem(
    tmp_path, monkeypatch, command, reply, problem
):
    with Simulator() as sim:
        handle = sim._handle
        monkeypatch.setattr(sim, "_handle", lambda c, p: reply if c == command else handle(c, p))
        printer = Printer(sim.connection, tmp_path / "operation.json")
        status = printer.probe()
        assert not status.ready
        assert problem in status.description
        with pytest.raises(ValueError, match=problem):
            printer.print_receipt(
                [Line("Olej", Decimal(1), Decimal("1.00"), 0)], [Payment("cash", 100)]
            )
        assert sim.receipts == []


def test_receipt_refuses_an_inactive_rate_and_one_that_changed_since_the_probe(
    tmp_path, monkeypatch
):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        printer.probe()
        with pytest.raises(ValueError, match="nieaktywna"):
            printer.print_receipt(
                [
                    Line("Olej", Decimal(1), Decimal("1.00"), 0),
                    Line("Inna usługa", Decimal(1), Decimal("1.00"), 3),  # vd = 101%
                ],
                [Payment("cash", 200)],
            )

        handle = sim._handle
        monkeypatch.setattr(
            sim,
            "_handle",
            lambda c, p: {**handle(c, p), "va": "8,00"} if c == "vatget" else handle(c, p),
        )
        with pytest.raises(ValueError, match="zmieniła się"):
            printer.print_receipt(
                [Line("Olej", Decimal(1), Decimal("1.00"), 0)], [Payment("cash", 100)]
            )
        assert sim.receipts == []


def test_receipt_arguments_are_checked_before_the_printer_is_touched(tmp_path):
    line = Line("Olej", Decimal(1), Decimal("1.00"), 0)
    biggest = Line("Olej", Decimal(1), Decimal("99999999.99"), 0)
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        for lines, payments, problem in [
            ([], [], "od 1 do 500"),
            ([line] * 501, [], "od 1 do 500"),
            ([{"name": "Olej"}], [], "Nieprawidłowe pozycje"),
            ([line], ["cash"], "płatności"),
            ([line], [0], "płatności"),
            ([line], [True], "płatności"),
            ([biggest] * 2, [], "przekracza zakres"),
        ]:
            with pytest.raises(ValueError, match=problem):
                printer.print_receipt(lines, payments)
        assert sim.receipts == []
        assert printer.last_record() is None


@pytest.mark.parametrize(
    ("command", "reply"),
    [
        ("getrealid", {"vr": "32.01", "nu": "DEMO0000001"}),  # no nm
        ("scomm", {"fs": "maybe", "hr": "1", "ts": "0"}),
        ("vatget", dict.fromkeys(("va", "vb", "vc", "vd", "ve", "vf", "vg"), "-")),
        ("vatget", dict.fromkeys(("va", "vb", "vc", "vd", "ve", "vf", "vg"), "Infinity")),
    ],
)
def test_a_garbled_status_reply_is_an_error_never_a_ready_printer(
    tmp_path, monkeypatch, command, reply
):
    with Simulator() as sim:
        handle = sim._handle
        monkeypatch.setattr(sim, "_handle", lambda c, p: reply if c == command else handle(c, p))
        printer = Printer(sim.connection, tmp_path / "operation.json")
        with pytest.raises(ProtocolError):
            printer.probe()
        assert printer.last_status is None
        with pytest.raises(ProtocolError):
            printer.print_receipt(
                [Line("Olej", Decimal(1), Decimal("1.00"), 0)], [Payment("cash", 100)]
            )
        assert sim.receipts == []


def test_acknowledging_on_the_same_ready_device_needs_no_force(tmp_path):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        printer._save(
            {
                "state": "pending",
                "operation": "receipt",
                "stage": "trend",
                "unique_number": "DEMO0000001",
            }
        )
        printer.acknowledge_pending()
        record = printer.last_record()
        assert record["state"] == "acknowledged"
        assert "resolution" not in record  # checked on the real device, not forced open
        printer.acknowledge_pending()  # nothing unresolved left: a no-op, not an error
        assert printer.last_record()["state"] == "acknowledged"


def test_unique_advance_slots_and_multiple_payments_use_exact_wire_frames(tmp_path, monkeypatch):
    from posnet.protocol import encode_frame

    lines = [
        Line("Usługa", Decimal(1), Decimal("10.00"), 0),
        Line("Część", Decimal(1), Decimal("5.00"), 1),
    ]
    advances = [Advance(0, 500), Advance(1, 100)]
    payments = [
        Payment("cash", 100),
        Payment("card", 200),
        Payment("transfer", 100),
        Payment("mobile", 200),
        Payment("credit", 300),
    ]
    with Simulator() as sim:
        frames = []
        handle = sim._handle

        def capture(command, params):
            frames.append(encode_frame(command, params))
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", capture)
        printer = Printer(sim.connection, tmp_path / "operation.json")
        record = printer.print_receipt(lines, payments, advances=advances)
        assert frames[frames.index(encode_frame("trinit", {"bm": "0"})) + 3 : -1] == [
            encode_frame("tradvance", {"na": "Zaliczka", "vt": 0, "pr": 500, "wa": 1000}),
            encode_frame("tradvance", {"na": "Zaliczka", "vt": 1, "pr": 100, "wa": 500}),
            encode_frame("trpayment", {"ty": 0, "wa": 100}),
            encode_frame("trpayment", {"ty": 2, "wa": 200}),
            encode_frame("trpayment", {"ty": 8, "wa": 100}),
            encode_frame("trpayment", {"ty": 6, "wa": 200, "na": "Płatność mobilna"}),
            encode_frame("trpayment", {"ty": 5, "wa": 300}),
            encode_frame("trend", {"to": 900, "fp": 900}),
        ]
        assert record["total_cents"] == sim.receipts[0]["total_cents"] == 900


@pytest.mark.parametrize("brand", ["", "Inna firma"])
def test_fully_prepaid_receipt_closes_at_zero_without_payment_commands(tmp_path, brand):
    lines = [Line("Usługa", Decimal(1), Decimal("10.00"), 0)]
    commands = receipt_commands(lines, [], [Advance(0, 1000)], brand)
    assert not any(command == "trpayment" for command, _, _ in commands)
    assert next(params for command, params, _ in commands if command == "trend") == (
        {"to": 0, "fp": 0} | ({"fe": "0"} if brand else {})
    )
    with Simulator() as sim:
        record = Printer(sim.connection, tmp_path / "operation.json").print_receipt(
            lines, [], brand, advances=[Advance(0, 1000)]
        )
        assert record["total_cents"] == 0
        assert sim.receipts[0]["payments"] == []


@pytest.mark.parametrize(
    ("advances", "payments"),
    [
        ([Advance(0, 80), Advance(0, 30)], []),
        ([Advance(0, 40), Advance(0, 10)], [Payment("cash", 50)]),
        ([Advance(1, 10)], [Payment("cash", 90)]),
        ([], [Payment("cash", 99)]),
        ([], [Payment("cash", 101)]),
    ],
)
def test_invalid_settlement_is_rejected_before_any_fiscal_command(tmp_path, advances, payments):
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        with pytest.raises(ValueError):
            printer.print_receipt(
                [Line("Usługa", Decimal(1), Decimal("1.00"), 0)], payments, advances=advances
            )
        assert not sim.transaction_open
        assert sim.receipts == []
        assert printer.last_record() is None


@pytest.mark.parametrize("amount", [-1, MAX_CENTS + 1, True, 1.5])
def test_settlement_amounts_are_nonnegative_bounded_cents(amount):
    with pytest.raises(ValueError):
        Payment("cash", amount)
    with pytest.raises(ValueError):
        Advance(0, amount)


@pytest.mark.parametrize("slot", [-1, 7, True, "0"])
def test_advance_requires_a_device_vat_slot(slot):
    with pytest.raises(ValueError):
        Advance(slot, 0)


def test_unknown_payment_form_is_rejected():
    with pytest.raises(ValueError):
        Payment("cheque", 100)
