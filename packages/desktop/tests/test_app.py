import copy
import json
import os
import threading
import time
from datetime import date
from types import SimpleNamespace

import pytest
from fiscal_desktop.app import MainWindow, Worker
from posnet.printer import Printer
from posnet.protocol import Connection
from posnet.simulator import Simulator
from PySide6.QtCore import QDate, QEventLoop, QObject, QTimer, Signal
from PySide6.QtTest import QSignalSpy, QTest
from PySide6.QtWidgets import QApplication, QLineEdit, QSystemTrayIcon
from test_agent import ATTEMPT, JOB, FakeApi


class PrinterEvents(QObject):
    printing = Signal()


def wait_for_signal(signal, spy):
    if spy.count():
        return True
    loop = QEventLoop()
    timer = QTimer()
    timer.setSingleShot(True)
    timer.timeout.connect(loop.quit)
    signal.connect(loop.quit)
    timer.start(5000)
    # Native Qt test waits can starve the Python QThread and socket simulator of the GIL.
    loop.exec()
    timer.stop()
    signal.disconnect(loop.quit)
    return bool(spy.count())


def wait_for(condition, window=None):
    deadline = time.monotonic() + 5
    while not condition() and time.monotonic() < deadline:
        QTest.qWait(5)
    assert condition(), window and window.message_label.text()


@pytest.fixture
def window(tmp_path):
    app = QApplication.instance() or QApplication([])
    view = MainWindow(Printer(Connection("/dev/null"), tmp_path / "operation.json"))
    view.show()
    yield view
    view.quit()
    wait_for(lambda: view.worker is None, view)
    view.deleteLater()
    QTest.qWait(1)
    assert QApplication.instance() is app


def test_setup_is_private_restored_and_does_not_print(window):
    window.api_url.setText("https://api.example")
    window.token.setText("secret")
    window.port.setCurrentText("/dev/null")
    assert window.save() is not None
    path = window.data / "fiscal.json"
    assert json.loads(path.read_text())["token"] == window.token.text()
    if os.name != "nt":
        assert path.stat().st_mode & 0o777 == 0o600
    assert window.worker is None
    assert window.token.echoMode() == QLineEdit.EchoMode.Password
    assert [window.tabs.tabText(i) for i in range(window.tabs.count())] == [
        "Do wydruku",
        "Raporty",
        "Historia",
        "Ustawienia",
    ]


@pytest.mark.parametrize("kind", ["advance", "sale"])
def test_desktop_queue_action_posts_selected_document_and_nip(window, monkeypatch, kind):
    queued = []

    class Commands:
        def submit(self, *args):
            queued.append(args)

    window.worker = Commands()
    window.connected = True
    allocation_id = ATTEMPT if kind == "advance" else None
    window.set_documents(
        [
            {
                "kind": kind,
                "orderId": JOB["orderId"],
                "orderNumber": JOB["orderNumber"],
                "allocationId": allocation_id,
                "amount": JOB["total"],
                "customerName": None,
                "occurredAt": JOB["createdAt"],
            }
        ],
        1,
        False,
    )
    assert window.documents_table.item(0, 0).text() == str(JOB["orderNumber"])
    assert window.documents_table.item(0, 1).text() == (
        "Zaliczka" if kind == "advance" else "Sprzedaż"
    )
    window.documents_table.selectRow(0)
    window.buyer_nip.setText("5260250274")
    monkeypatch.setattr(window, "_confirm", lambda *a: True)
    try:
        window.submit_receipt()
        assert queued == [("queue", kind, JOB["orderId"], allocation_id, "5260250274")]
    finally:
        window.worker = None


@pytest.mark.parametrize("tray_available", [True, False])
def test_close_hides_only_with_tray(window, monkeypatch, tray_available):
    monkeypatch.setattr(QSystemTrayIcon, "isSystemTrayAvailable", lambda: tray_available)
    window.close()
    assert window.isVisible() is not tray_available
    window.open_window()
    assert window.isVisible()


def test_report_controls_preserve_dates_confirmation_and_summary(window, monkeypatch):
    commands = []

    class Commands:
        def submit(self, *args, **kwargs):
            commands.append((args, kwargs))

    window.worker = Commands()
    monkeypatch.setattr(window, "_confirm", lambda *a: True)
    try:
        previous = QDate.currentDate().addMonths(-1)
        assert window.month_picker.currentData() == (previous.year(), previous.month())
        window.submit_daily_report()
        window.submit_monthly_report()
        window.period_start.setDate(window.period_end.date().addDays(-3))
        window.period_summary.setChecked(True)
        window.submit_periodic_report()
        assert commands[0] == (("daily_report",), {})
        assert commands[1] == (("monthly_report", previous.year(), previous.month()), {})
        args, kwargs = commands[2]
        assert args[0] == "periodic_report" and isinstance(args[1], date)
        assert (args[2] - args[1]).days == 3 and kwargs == {"summary": True}
        window.period_end.setDate(window.period_start.date().addDays(-1))
        window.submit_periodic_report()
        assert len(commands) == 3
    finally:
        window.worker = None


def test_history_lists_receipts_newest_first(window):
    window.history_path.write_text(
        "\n".join(
            json.dumps(
                {
                    "job_id": str(i),
                    "timestamp": f"2026-10-{i:02}T10:00:00Z",
                    "receipt_number": str(i),
                    "total_cents": 100,
                    "payments": [
                        {"form": "card", "amount_cents": 40},
                        {"form": "credit", "amount_cents": 60},
                    ],
                    "lines": [{"name": "Olej"}],
                }
            )
            for i in (1, 2)
        )
    )
    window.load_history()
    assert [window.history_table.item(row, 2).text() for row in range(2)] == ["2", "1"]
    assert window.history_table.item(0, 4).text() == ("Karta płatnicza: 0,40 zł, Kredyt: 0,60 zł")


def test_web_job_prints_in_single_background_worker_and_stop_waits(tmp_path, monkeypatch):
    app = QApplication.instance() or QApplication([])
    events = PrinterEvents()
    printing = QSignalSpy(events.printing)
    finish = threading.Event()
    api = FakeApi([JOB])
    api.connect = lambda: None
    api.documents = lambda cursor=None: {"items": [], "nextCursor": None}
    with Simulator() as sim:
        handle = sim._handle

        def gate(command, params):
            if command == "trend":
                events.printing.emit()
                assert finish.wait(5)
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", gate)
        monkeypatch.setattr("fiscal_desktop.app.Api", lambda *_: api)
        view = MainWindow(Printer(sim.connection, tmp_path / "operation.json"))
        view.api_url.setText("https://api.example")
        view.token.setText("secret")
        view.port.setCurrentText(sim.connection.address)
        view.start()
        worker = view.worker
        stopped = QSignalSpy(worker.finished)
        try:
            assert wait_for_signal(events.printing, printing), view.message_label.text()
            view.stop()
            assert worker.isRunning() and worker.isInterruptionRequested()
        finally:
            finish.set()
            view.stop()
            assert wait_for_signal(worker.finished, stopped), view.message_label.text()
            assert view.worker is None
            view.quit()
        assert len(sim.receipts) == 1
        assert api.reports[0]["state"] == "done"
        view.load_history()
        assert view.history_table.rowCount() == 1
        view.deleteLater()
    assert QApplication.instance() is app


def test_reports_and_receipts_are_serialized_by_one_worker(tmp_path, monkeypatch):
    app = QApplication.instance() or QApplication([])
    api = FakeApi([JOB])
    api.connect = lambda: None
    api.documents = lambda cursor=None: {"items": [], "nextCursor": None}
    monkeypatch.setattr("fiscal_desktop.app.Api", lambda *_: api)
    with Simulator() as sim:
        printer = Printer(sim.connection, tmp_path / "operation.json")
        config = {"api_url": "https://api.example", "token": "secret"}
        worker = Worker(printer, config)
        printed = QSignalSpy(worker.history)
        stopped = QSignalSpy(worker.finished)
        worker.submit("daily_report")
        worker.start()
        try:
            assert wait_for_signal(worker.history, printed)
            worker.stop()
            assert wait_for_signal(worker.finished, stopped)
            assert len(sim.receipts) == 1
            assert [report["command"] for report in sim.reports] == ["dailyrep"]
            assert printer.last_record()["state"] == "acknowledged"
        finally:
            worker.stop()
            worker.wait(5000)
    assert QApplication.instance() is app


def test_legacy_agent_state_keeps_journal_and_outbox_together(tmp_path):
    from fiscal_desktop.__main__ import state_directory

    legacy = tmp_path / "workshop-agent"
    legacy.mkdir()
    (legacy / "fiscal-operation.json").write_text('{"state":"pending"}')
    (legacy / "fiscal-operation.agent.json").write_text('{"settled":false}')
    data = state_directory(tmp_path)
    assert data.name == "holuto-fiscal"
    assert json.loads((data / "fiscal-operation.json").read_text())["state"] == "pending"
    assert not json.loads((data / "fiscal-operation.agent.json").read_text())["settled"]
    assert not legacy.exists()


def test_ambiguous_legacy_state_is_never_silently_discarded(tmp_path):
    from fiscal_desktop.__main__ import state_directory

    for name in ("holuto-fiscal", "workshop-agent"):
        (tmp_path / name).mkdir()
    with pytest.raises(ValueError, match="kilka rejestrów"):
        state_directory(tmp_path)
    assert (tmp_path / "workshop-agent").exists()


def test_document_selection_distinguishes_advances_for_the_same_order(window):
    items = [
        {
            "kind": "advance",
            "orderId": JOB["orderId"],
            "orderNumber": 10,
            "allocationId": allocation_id,
            "amount": "10.00",
            "customerName": None,
            "occurredAt": JOB["createdAt"],
        }
        for allocation_id in (ATTEMPT, JOB["id"])
    ]
    window.set_documents(items, 1, False)
    window.documents_table.selectRow(1)
    window.set_documents(items[::-1], 1, False)
    assert window.selected_document()["allocationId"] == JOB["id"]


def page_document(order_number):
    return {
        "kind": "sale",
        "orderId": f"00000000-0000-0000-0000-{order_number:012d}",
        "orderNumber": order_number,
        "allocationId": None,
        "amount": "10.00",
        "customerName": None,
        "occurredAt": JOB["createdAt"],
    }


class PagedApi:
    """`/agent/documents` by cursor; a cursor in `failing` loses its response."""

    def __init__(self, pages):
        self.pages = pages
        self.cursors = []
        self.failing = set()

    def documents(self, cursor=None):
        self.cursors.append(cursor)
        if cursor in self.failing:
            raise OSError("page lost")
        return copy.deepcopy(self.pages[cursor])


def paged_worker(pages):
    worker = Worker(None, {})
    worker.agent = SimpleNamespace(api=PagedApi(pages))
    shown = []
    worker.documents.connect(lambda *page: shown.append(page))
    return worker, worker.agent.api, shown


def navigate(worker, command):
    # The poll loop's order: queued commands first, then the documents refresh.
    worker.submit(command)
    worker._commands()
    worker._refresh_documents()


def test_each_poll_fetches_only_the_shown_page_and_buttons_page_through():
    first, second = page_document(1), page_document(2)
    worker, api, shown = paged_worker(
        {
            None: {"items": [first], "nextCursor": "after-1"},
            "after-1": {"items": [second], "nextCursor": None},
        }
    )
    worker._refresh_documents()
    worker._refresh_documents()  # a later poll refreshes the same single page
    assert shown == [([first], 1, True)] * 2
    navigate(worker, "next_documents")
    assert shown[-1] == ([second], 2, False)
    navigate(worker, "next_documents")  # no next page: stays put
    assert shown[-1] == ([second], 2, False)
    navigate(worker, "previous_documents")
    assert shown[-1] == ([first], 1, True)
    assert api.cursors == [None, None, "after-1", "after-1", None]


def test_failed_later_page_keeps_the_shown_page_and_its_cursor():
    first = page_document(1)
    worker, api, shown = paged_worker({None: {"items": [first], "nextCursor": "after-1"}})
    worker._refresh_documents()
    api.failing.add("after-1")
    with pytest.raises(OSError, match="page lost"):
        navigate(worker, "next_documents")
    assert shown == [([first], 1, True)]
    assert worker.document_pages == [None]
    assert worker.next_document_cursor == "after-1"
    worker._refresh_documents()  # the next poll shows page 1 again, not a half-loaded page 2
    assert shown[-1] == ([first], 1, True)
    assert api.cursors == [None, "after-1", None]


def test_emptied_later_page_returns_to_the_first():
    first = page_document(1)
    worker, api, shown = paged_worker(
        {
            None: {"items": [first], "nextCursor": "after-1"},
            "after-1": {"items": [], "nextCursor": None},  # everything there was queued
        }
    )
    worker._refresh_documents()
    navigate(worker, "next_documents")
    assert shown[-1] == ([first], 1, True)
    assert worker.document_pages == [None]
    assert api.cursors == [None, "after-1", None]


def test_page_controls_follow_the_shown_page(window):
    commands = []

    class Commands:
        def submit(self, *args):
            commands.append(args)

    window.worker = Commands()
    window.connected = True
    try:
        window.set_documents([page_document(1)], 1, True)
        assert not window.previous_documents_button.isEnabled()
        assert window.next_documents_button.isEnabled()
        window.next_documents_button.click()
        window.set_documents([page_document(2)], 2, False)
        assert window.documents_page_label.text() == "Strona 2"
        assert window.previous_documents_button.isEnabled()
        assert not window.next_documents_button.isEnabled()
        window.previous_documents_button.click()
        assert commands == [("next_documents",), ("previous_documents",)]
    finally:
        window.worker = None


@pytest.mark.parametrize(
    ("serial", "baudrate", "expected"),
    [
        (None, None, ("SAVED", 38400)),
        ("OVERRIDE", None, ("OVERRIDE", 38400)),
        (None, 19200, ("SAVED", 19200)),
        ("OVERRIDE", 19200, ("OVERRIDE", 19200)),
    ],
)
def test_saved_connection_and_cli_overrides_reach_controls_and_worker(
    tmp_path, monkeypatch, serial, baudrate, expected
):
    from argparse import Namespace

    from fiscal_desktop.__main__ import configured_printer

    app = QApplication.instance() or QApplication([])
    (tmp_path / "fiscal.json").write_text(
        json.dumps(
            {
                "serial": "SAVED",
                "baudrate": 38400,
                "api_url": "https://api.example",
                "token": "secret",
            }
        )
    )
    monkeypatch.setattr(Worker, "start", lambda _: None)
    printer = configured_printer(Namespace(data_dir=tmp_path, serial=serial, baudrate=baudrate))
    view = MainWindow(printer)
    try:
        assert (printer.connection.address, printer.connection.baudrate) == expected
        assert (view.port.currentText(), int(view.baudrate.currentText())) == expected
        view.start()
        connection = view.worker.printer.connection
        assert (connection.address, connection.baudrate) == expected
    finally:
        if view.worker is not None:
            view.worker.deleteLater()
            view.worker = None
        view.quit()
        view.deleteLater()
    assert QApplication.instance() is app
