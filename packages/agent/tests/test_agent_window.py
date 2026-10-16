import json
import os
import threading

import pytest
from posnet.simulator import Simulator
from PySide6.QtCore import QEventLoop, QObject, Qt, QTimer, Signal
from PySide6.QtTest import QSignalSpy, QTest
from PySide6.QtWidgets import QApplication, QLineEdit, QSystemTrayIcon
from test_agent import JOB
from test_api import server, session
from workshop_agent.app import AgentWindow


class PrinterEvents(QObject):
    printing = Signal()


class ObservedAgentWindow(AgentWindow):
    stopped = Signal()

    def finished(self):
        super().finished()
        self.stopped.emit()


def wait_for_signal(signal, spy):
    if spy.count():
        return True
    loop = QEventLoop()
    timer = QTimer()
    timer.setSingleShot(True)
    timer.timeout.connect(loop.quit)
    signal.connect(loop.quit)
    timer.start(5000)
    # QSignalSpy.wait holds the GIL here, preventing the Python QThread from starting.
    loop.exec()
    timer.stop()
    signal.disconnect(loop.quit)
    return bool(spy.count())


def test_saved_setup_does_not_start_printing(tmp_path):
    app = QApplication.instance() or QApplication([])
    with Simulator() as sim, server([]) as (url, requests):
        window = AgentWindow(tmp_path)
        window.api_url.setText(url)
        window.token.setText("secret")
        window.port.setCurrentText(sim.connection.address)
        QTest.mouseClick(window.save_button, Qt.MouseButton.LeftButton)
        assert QApplication.instance() is app
        if os.name != "nt":
            assert (tmp_path / "fiscal.json").stat().st_mode & 0o777 == 0o600
        assert requests == []
        assert sim.receipts == []
        assert window.worker is None
        window.quit()
        window.deleteLater()
        window = AgentWindow(tmp_path)
        assert window.api_url.text() == url
        assert window.token.text() == "secret"
        assert window.token.echoMode() == QLineEdit.EchoMode.Password
        assert window.port.currentText() == sim.connection.address
        window.quit()
        window.deleteLater()


@pytest.mark.parametrize("tray_available", [True, False])
def test_close_keeps_running_and_quit_finishes_receipt(tmp_path, monkeypatch, tray_available):
    app = QApplication.instance() or QApplication([])
    monkeypatch.setattr(QSystemTrayIcon, "isSystemTrayAvailable", lambda: tray_available)
    events = PrinterEvents()
    printing = QSignalSpy(events.printing)
    finish = threading.Event()
    done = {**JOB, "state": "done", "receiptNumber": "1", "resolvedAt": JOB["takenAt"]}
    with (
        Simulator() as sim,
        server([session(), session(), (200, JOB, {}), (200, done, {})]) as (url, requests),
    ):
        handle = sim._handle

        def gate(command, params):
            if command == "trend":
                events.printing.emit()
                assert finish.wait(5)
            return handle(command, params)

        monkeypatch.setattr(sim, "_handle", gate)
        window = ObservedAgentWindow(tmp_path)
        window.show()
        window.api_url.setText(url)
        window.token.setText("secret")
        window.port.setCurrentText(sim.connection.address)
        stopped = QSignalSpy(window.stopped)
        QTest.mouseClick(window.start_button, Qt.MouseButton.LeftButton)
        try:
            assert wait_for_signal(events.printing, printing), (window.status.text(), requests)
            assert QApplication.instance() is app
            window.close()
            assert window.isVisible() is not tray_available
            assert not window.worker.isInterruptionRequested()
            if tray_available:
                window.tray.contextMenu().actions()[0].trigger()
                assert window.isVisible()
                window.close()
                window.tray.contextMenu().actions()[-1].trigger()
            else:
                QTest.mouseClick(window.quit_button, Qt.MouseButton.LeftButton)
            assert window.worker.isRunning()
            assert window.worker.isInterruptionRequested()
        finally:
            finish.set()
            window.stop()
            assert wait_for_signal(window.stopped, stopped)
        assert window.worker is None
        assert not window.isVisible()
        assert len(sim.receipts) == 1
        assert requests[-1][2]["state"] == "done"
        assert requests[-1][2]["receiptNumber"] == "1"
        assert requests[-1][2]["attempt"] == JOB["attempt"]
        assert (
            json.loads((tmp_path / "fiscal-operation.json").read_text())["state"] == "acknowledged"
        )
