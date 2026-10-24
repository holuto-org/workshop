"""Holuto-connected UI; one background thread owns every printer operation."""

import json
import logging
from http import HTTPStatus
from pathlib import Path
from queue import Empty, Queue
from threading import Event
from urllib.error import HTTPError

from posnet.models import money
from posnet.printer import Printer
from posnet.protocol import Connection, ProtocolError
from PySide6.QtCore import QDate, QLocale, QSettings, Qt, QThread, Signal
from PySide6.QtGui import QIcon
from PySide6.QtWidgets import (
    QAbstractItemView,
    QApplication,
    QCheckBox,
    QComboBox,
    QDateEdit,
    QDialog,
    QFormLayout,
    QFrame,
    QHBoxLayout,
    QHeaderView,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMenu,
    QPushButton,
    QSystemTrayIcon,
    QTableWidget,
    QTableWidgetItem,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)
from serial.tools import list_ports

from .agent import (
    POLL_SECONDS,
    RETRY_SECONDS,
    Agent,
    Api,
    load_config,
    log,
    save_config,
    validated_url,
)

PAYMENTS = {
    "cash": "Gotówka",
    "card": "Karta płatnicza",
    "transfer": "Przelew",
    "mobile": "Płatność mobilna",
    "credit": "Kredyt",
}
KINDS = {"advance": "Zaliczka", "sale": "Sprzedaż"}
HISTORY_LIMIT = 500
BUYER_NIP_LENGTH = 10
RIGHT = Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter
REPORTS = frozenset({"daily_report", "monthly_report", "periodic_report", "acknowledge_pending"})


def settings():
    return QSettings("Holuto", "Fiscal")


def migrate_legacy_settings():
    current = settings()
    if not current.allKeys():
        legacy = QSettings("HuggingCar", "Fiscal")
        for key in legacy.allKeys():
            current.setValue(key, legacy.value(key))


def saved_connection(data):
    config = load_config(data / "fiscal.json")
    saved = settings()
    try:
        baudrate = int(config.get("baudrate", saved.value("connection/baudrate", 9600)))
    except TypeError, ValueError:
        baudrate = 9600
    return Connection(
        address=config.get("serial", saved.value("connection/address", "")), baudrate=baudrate
    )


def serial_ports():
    return sorted((port.device for port in list_ports.comports()), key=lambda d: "ttyACM" not in d)


def label(text, name):
    widget = QLabel(text)
    widget.setObjectName(name)
    return widget


def set_state(widget, state):
    if widget.property("state") != state:
        widget.setProperty("state", state)
        widget.style().unpolish(widget)
        widget.style().polish(widget)


def dialog_footer(dialog, accept_text):
    footer = QHBoxLayout()
    footer.addStretch()
    for text, slot in (("Anuluj", dialog.reject), (accept_text, dialog.accept)):
        button = QPushButton(text)
        button.setAutoDefault(False)
        if text == accept_text:
            button.setObjectName("okButton")
            button.setDefault(True)
        button.clicked.connect(slot)
        footer.addWidget(button)
    return footer


class StatusLog(logging.Handler):
    def __init__(self, signal):
        super().__init__()
        self.signal = signal

    def emit(self, record):
        self.signal.emit(self.format(record), "error" if record.levelno >= logging.WARNING else "")


class Worker(QThread):
    status = Signal(str, str)
    documents = Signal(list, int, bool)  # one page: items, page number, whether a next exists
    device = Signal(object, bool)
    connected = Signal()
    history = Signal()

    def __init__(self, printer, config, parent=None):
        super().__init__(parent)
        self.printer = printer
        self.config = config
        self.agent = None
        self.commands = Queue()
        self.wake = Event()
        # Discovery shows one bounded page per poll. `document_pages` holds the cursor that
        # opened each page up to the shown one (None: the first); a requested page replaces it
        # only once it loaded, so a failed page keeps the previous one on screen.
        self.document_pages = [None]
        self.next_document_cursor = None
        self.requested_pages = None

    def submit(self, method, *args, **kwargs):
        self.commands.put((method, args, kwargs))
        self.wake.set()

    def stop(self):
        self.requestInterruption()
        self.wake.set()

    def run(self):
        handler = StatusLog(self.status)
        log.addHandler(handler)
        try:
            device = self.printer.detect(serial_ports())
            api = Api(self.config["api_url"], self.config["token"], device.unique_number)
            api.describe(device)
            api.connect()
            self.agent = Agent(self.printer, api)
            self.connected.emit()
            self.status.emit(
                f"Połączono: {device.unique_number}. Oczekiwanie na zlecenia.", "success"
            )
            self._loop()
        except Exception as exc:  # noqa: BLE001 - report failures at the Qt thread boundary.
            self.status.emit(f"Błąd: {exc}", "error")
        finally:
            log.removeHandler(handler)

    def _loop(self):
        while not self.isInterruptionRequested():
            self.wake.clear()
            delay = POLL_SECONDS
            try:
                self._commands()
                if self.isInterruptionRequested():
                    return
                delay = self.agent.step() or 0
                if self.isInterruptionRequested():
                    return
                self._refresh_documents()
                self._publish_device()
                self.history.emit()
            except HTTPError as exc:
                if exc.code == HTTPStatus.UNAUTHORIZED:
                    raise
                self.status.emit(f"Błąd API: {exc}", "error")
                delay = RETRY_SECONDS
            except (OSError, ProtocolError, ValueError, LookupError, TypeError) as exc:
                self.status.emit(f"Błąd: {exc}", "error")
                self._publish_device()
                delay = RETRY_SECONDS
            self.wake.wait(delay)

    def _publish_device(self):
        self.device.emit(self.printer.last_status, self.agent.blocked)

    def _commands(self):
        while not self.isInterruptionRequested():
            try:
                method, args, kwargs = self.commands.get_nowait()
            except Empty:
                return
            if method == "queue":
                self.agent.api.queue(*args, **kwargs)
                self.status.emit("Zlecenie dodano do kolejki Holuto.", "success")
            elif method == "next_documents":
                if self.next_document_cursor is not None:
                    self.requested_pages = [*self.document_pages, self.next_document_cursor]
            elif method == "previous_documents":
                self.requested_pages = self.document_pages[:-1] or [None]
            elif method in REPORTS:
                self._report(method, args, kwargs)
            else:
                raise ValueError("Nieznana operacja drukarki.")

    def _refresh_documents(self):
        pages, self.requested_pages = self.requested_pages or self.document_pages, None
        page = self.agent.api.documents(pages[-1])
        if not page["items"] and len(pages) > 1:
            # Everything from this page on was printed or queued: start over.
            pages = [None]
            page = self.agent.api.documents()
        self.document_pages, self.next_document_cursor = pages, page["nextCursor"]
        self.documents.emit(page["items"], len(pages), page["nextCursor"] is not None)

    def _report(self, method, args, kwargs):
        self.agent.run_report(method, *args, **kwargs)
        self.status.emit(
            "Raport wydrukowany." if method != "acknowledge_pending" else "Wyjaśniono raport.",
            "success",
        )


class MainWindow(QMainWindow):
    def __init__(self, printer: Printer):
        super().__init__()
        self.printer = printer
        self.data = printer.journal_path.parent
        self.history_path = self.data / "history.jsonl"
        self.worker = None
        self.connected = False
        self.closing = False
        self.status = None
        self.blocked = False
        self.document_items = []
        self.documents_page = 1
        self.documents_have_next = False
        self.setWindowTitle("Holuto Fiscal")
        self.setWindowIcon(QIcon(str(Path(__file__).with_name("icon.svg"))))
        self.resize(1180, 820)
        theme = Path(__file__).with_name("theme.qss")
        self.setStyleSheet(
            theme.read_text(encoding="utf-8").replace("{dir}", theme.parent.as_posix())
        )
        self._build_ui(load_config(self.data / "fiscal.json"))
        self._build_tray()
        self._update_controls()

    @staticmethod
    def _card(title=None):
        card = QFrame()
        card.setObjectName("card")
        layout = QVBoxLayout(card)
        layout.setContentsMargins(24, 20, 24, 24)
        layout.setSpacing(16)
        if title:
            layout.addWidget(label(title, "cardTitle"))
        return card, layout

    @staticmethod
    def _page():
        page = QWidget()
        page.setObjectName("page")
        layout = QHBoxLayout(page)
        layout.setContentsMargins(0, 16, 0, 0)
        layout.setSpacing(16)
        return page, layout

    @staticmethod
    def _table(headers, widths, numeric=()):
        table = QTableWidget(0, len(headers))
        table.setHorizontalHeaderLabels(headers)
        table.horizontalHeader().setDefaultAlignment(
            Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignVCenter
        )
        for col in numeric:
            table.horizontalHeaderItem(col).setTextAlignment(RIGHT)
        table.verticalHeader().hide()
        table.verticalHeader().setDefaultSectionSize(40)
        table.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeMode.Stretch)
        for col, width in widths.items():
            table.horizontalHeader().setSectionResizeMode(col, QHeaderView.ResizeMode.Fixed)
            table.setColumnWidth(col, width)
        table.setSelectionBehavior(QAbstractItemView.SelectionBehavior.SelectRows)
        table.setSelectionMode(QAbstractItemView.SelectionMode.SingleSelection)
        table.setEditTriggers(QAbstractItemView.EditTrigger.NoEditTriggers)
        table.setShowGrid(False)
        return table

    def _build_ui(self, config):
        root = QWidget()
        root.setObjectName("root")
        self.setCentralWidget(root)
        layout = QVBoxLayout(root)
        layout.setContentsMargins(24, 20, 24, 16)
        self.signin = label(
            "Połącz Holuto, aby pobierać zlecenia i drukować paragony.", "cardTitle"
        )
        self.tabs = QTabWidget()
        self.tabs.setDocumentMode(True)
        self.tabs.addTab(self._build_documents(), "Do wydruku")
        self.tabs.addTab(self._build_reports(), "Raporty")
        self.tabs.addTab(self._build_history(), "Historia")
        self.tabs.addTab(self._build_settings(config), "Ustawienia")
        self.tabs.currentChanged.connect(lambda _: self.load_history())
        layout.addWidget(self.signin)
        layout.addWidget(self.tabs, 1)
        footer = QHBoxLayout()
        self.message_label = label("Zaloguj się tokenem drukarki z Holuto.", "message")
        self.message_label.setWordWrap(True)
        footer.addWidget(self.message_label, 1)
        self.recovery_button = QPushButton("Wyjaśnij raport")
        self.recovery_button.clicked.connect(self.resolve_report)
        self.recovery_button.hide()
        footer.addWidget(self.recovery_button)
        layout.addLayout(footer)
        self.tabs.setCurrentIndex(3)

    def _build_documents(self):
        page, body = self._page()
        card, layout = self._card("Dokumenty oczekujące na paragon")
        self.documents_table = self._table(
            ["Zlecenie", "Rodzaj", "Klient", "Kwota", "Data"],
            {1: 110, 2: 180, 3: 120, 4: 170},
            (3,),
        )
        self.documents_table.itemSelectionChanged.connect(self._update_controls)
        layout.addWidget(self.documents_table)
        pager = QHBoxLayout()
        self.previous_documents_button = QPushButton("Poprzednia strona")
        self.previous_documents_button.clicked.connect(lambda: self._submit("previous_documents"))
        self.documents_page_label = label("Strona 1", "muted")
        self.next_documents_button = QPushButton("Następna strona")
        self.next_documents_button.clicked.connect(lambda: self._submit("next_documents"))
        pager.addWidget(self.previous_documents_button)
        pager.addStretch()
        pager.addWidget(self.documents_page_label)
        pager.addStretch()
        pager.addWidget(self.next_documents_button)
        layout.addLayout(pager)
        form = QFormLayout()
        self.buyer_nip = QLineEdit()
        self.buyer_nip.setPlaceholderText("Opcjonalny NIP nabywcy — 10 cyfr")
        self.buyer_nip.setMaxLength(10)
        form.addRow("NIP nabywcy", self.buyer_nip)
        layout.addLayout(form)
        self.print_button = QPushButton("Dodaj do wydruku")
        self.print_button.setObjectName("primaryButton")
        self.print_button.clicked.connect(self.submit_receipt)
        layout.addWidget(self.print_button)
        layout.addWidget(
            label(
                "Zadania z aplikacji webowej drukują się automatycznie. "
                "Niepotwierdzone wyniki rozstrzyga personel w Holuto.",
                "muted",
            )
        )
        body.addWidget(card)
        return page

    def _build_settings(self, config):
        page, body = self._page()
        card, layout = self._card("Połączenie z Holuto")
        self.form = QWidget()
        form = QFormLayout(self.form)
        self.api_url = QLineEdit(config.get("api_url", ""))
        self.api_url.setPlaceholderText("https://api.example.com")
        self.token = QLineEdit(config.get("token", ""))
        self.token.setEchoMode(QLineEdit.EchoMode.Password)
        self.token.setPlaceholderText("Token parowania z Holuto")
        self.port = QComboBox()
        self.port.setEditable(True)
        self.port.addItems(serial_ports())
        self.port.setCurrentText(self.printer.connection.address)
        self.baudrate = QComboBox()
        self.baudrate.addItems(["9600", "19200", "38400", "57600", "115200"])
        self.baudrate.setCurrentText(str(self.printer.connection.baudrate))
        for title, widget in (
            ("Adres API", self.api_url),
            ("Token parowania", self.token),
            ("Port drukarki", self.port),
            ("Prędkość transmisji", self.baudrate),
        ):
            form.addRow(title, widget)
        layout.addWidget(self.form)
        actions = QHBoxLayout()
        self.save_button = QPushButton("Zapisz")
        self.start_button = QPushButton("Zaloguj i uruchom")
        self.start_button.setObjectName("primaryButton")
        self.stop_button = QPushButton("Zatrzymaj")
        self.quit_button = QPushButton("Zakończ")
        for button, slot in (
            (self.save_button, self.save),
            (self.start_button, self.start),
            (self.stop_button, self.stop),
            (self.quit_button, self.quit),
        ):
            button.clicked.connect(slot)
            actions.addWidget(button)
        layout.addLayout(actions)
        layout.addWidget(
            label(
                "Zamknięcie okna ukrywa aplikację w zasobniku. "
                "Aby zmienić ustawienia, najpierw zatrzymaj obsługę drukarki.",
                "muted",
            )
        )
        body.addWidget(card, 1, Qt.AlignmentFlag.AlignTop)
        return page

    def _report_row(self, layout, title, description, *controls):
        row = QHBoxLayout()
        text = QVBoxLayout()
        text.addWidget(QLabel(title))
        hint = label(description, "muted")
        hint.setWordWrap(True)
        text.addWidget(hint)
        row.addLayout(text, 1)
        for control in controls:
            row.addWidget(control)
        layout.addLayout(row)

    @staticmethod
    def _divider(layout):
        line = QFrame()
        line.setObjectName("divider")
        line.setFixedHeight(1)
        layout.addWidget(line)

    def _build_reports(self):
        page, body = self._page()
        today = QDate.currentDate()
        card, layout = self._card("Raporty fiskalne")
        self.daily_button = QPushButton("Drukuj raport dobowy")
        self.daily_button.clicked.connect(self.submit_daily_report)
        self._report_row(
            layout,
            "Raport dobowy",
            "Zamyka sprzedaż bieżącego dnia w pamięci fiskalnej. Wykonuj po zakończeniu sprzedaży.",
            self.daily_button,
        )
        self._divider(layout)
        self.month_picker = QComboBox()
        locale = QLocale(QLocale.Language.Polish)
        for back in range(1, 25):
            month = today.addMonths(-back)
            self.month_picker.addItem(
                f"{locale.standaloneMonthName(month.month())} {month.year()}",
                (month.year(), month.month()),
            )
        self.monthly_button = QPushButton("Drukuj raport miesięczny")
        self.monthly_button.clicked.connect(self.submit_monthly_report)
        self._report_row(
            layout,
            "Raport miesięczny",
            "Łączny raport fiskalny za wybrany miesiąc.",
            self.month_picker,
            self.monthly_button,
        )
        self._divider(layout)
        self.period_start = QDateEdit(today.addDays(-7))
        self.period_end = QDateEdit(today)
        for picker in (self.period_start, self.period_end):
            picker.setDisplayFormat("dd.MM.yyyy")
            picker.setCalendarPopup(True)
            picker.setMaximumDate(today)
            picker.setFixedWidth(140)
        self.period_summary = QCheckBox("Skrócony")
        self.periodic_button = QPushButton("Drukuj raport okresowy")
        self.periodic_button.clicked.connect(self.submit_periodic_report)
        self._report_row(
            layout,
            "Raport okresowy",
            "Za dowolny zakres dat: szczegółowy lub skrócony (łączny).",
            self.period_start,
            label("–", "secondary"),
            self.period_end,
            self.period_summary,
            self.periodic_button,
        )
        body.addWidget(card, 1, Qt.AlignmentFlag.AlignTop)
        return page

    def _build_history(self):
        page, body = self._page()
        card, layout = self._card("Wydrukowane paragony")
        self.history_table = self._table(
            ["Pozycje", "Data", "Nr paragonu", "Kwota", "Płatność"],
            {1: 150, 2: 110, 3: 120, 4: 130},
            (3,),
        )
        layout.addWidget(self.history_table)
        layout.addWidget(
            label(
                "Lista z tego komputera. Numer nadaje drukarka; "
                "pamięć fiskalna jest źródłem prawdy.",
                "muted",
            )
        )
        body.addWidget(card)
        return page

    def load_history(self):
        try:
            rows = [
                json.loads(line)
                for line in self.history_path.read_text(encoding="utf-8").splitlines()
                if line
            ]
        except FileNotFoundError:
            rows = []
        except (OSError, ValueError) as exc:
            self._say(f"Nie można odczytać historii: {exc}", "error")
            return
        rows = rows[-HISTORY_LIMIT:][::-1]
        self.history_table.setRowCount(len(rows))
        for row, record in enumerate(rows):
            payments = ", ".join(
                f"{PAYMENTS.get(item['form'], '—')}: {money(item['amount_cents'])}"
                for item in record.get("payments", [])
            )
            values = [
                ", ".join(line["name"] for line in record.get("lines", [])),
                record.get("timestamp", "")[:16].replace("T", " "),
                record.get("receipt_number") or "—",
                money(record.get("total_cents", 0)),
                payments or "—",
            ]
            for col, value in enumerate(values):
                self.history_table.setItem(row, col, QTableWidgetItem(value))

    def set_documents(self, items, page, has_next):
        selected = self.selected_document()
        self.document_items = items
        self.documents_page, self.documents_have_next = page, has_next
        self.documents_page_label.setText(f"Strona {page}")
        self.documents_table.setRowCount(len(items))
        for row, document in enumerate(items):
            values = [
                str(document["orderNumber"]),
                KINDS[document["kind"]],
                document["customerName"] or "—",
                document["amount"] + " zł",
                document["occurredAt"][:16].replace("T", " "),
            ]
            for col, value in enumerate(values):
                self.documents_table.setItem(row, col, QTableWidgetItem(value))
            if selected and all(
                selected[key] == document[key] for key in ("kind", "orderId", "allocationId")
            ):
                self.documents_table.selectRow(row)
        self._update_controls()

    def selected_document(self):
        row = self.documents_table.currentRow()
        return self.document_items[row] if 0 <= row < len(self.document_items) else None

    def _update_controls(self):
        running = self.worker is not None
        self.form.setEnabled(not running)
        self.save_button.setEnabled(not running)
        self.start_button.setEnabled(not running)
        self.stop_button.setEnabled(running)
        self.print_button.setEnabled(
            self.connected and bool(self.selected_document()) and not self.blocked
        )
        self.previous_documents_button.setEnabled(self.connected and self.documents_page > 1)
        self.next_documents_button.setEnabled(self.connected and self.documents_have_next)
        for button in (self.daily_button, self.monthly_button, self.periodic_button):
            button.setEnabled(
                self.connected and bool(self.status and self.status.ready) and not self.blocked
            )
        self.signin.setVisible(not self.connected)

    def _say(self, text, state=""):
        self.message_label.setText(text)
        set_state(self.message_label, state)

    def save(self):
        token = self.token.text().strip()
        if (
            not token
            or not token.isascii()
            or not token.isprintable()
            or any(c.isspace() for c in token)
        ):
            self._say("Wklej token drukarki z Holuto.", "error")
            return None
        try:
            config = {
                "api_url": validated_url(self.api_url.text().strip()),
                "token": token,
                "serial": self.port.currentText().strip(),
                "baudrate": int(self.baudrate.currentText()),
            }
            save_config(self.data / "fiscal.json", config)
        except (OSError, ValueError) as exc:
            self._say(f"Błąd: {exc}", "error")
            return None
        self._say("Zapisano ustawienia.", "success")
        return config

    def start(self):
        if self.worker is not None:
            return
        config = self.save()
        if config is None:
            return
        self.printer = Printer(
            Connection(config["serial"], config["baudrate"]), self.printer.journal_path
        )
        self.worker = Worker(self.printer, config, self)
        self.worker.status.connect(self._say)
        self.worker.documents.connect(self.set_documents)
        self.worker.device.connect(self.set_device)
        self.worker.connected.connect(self._connected)
        self.worker.history.connect(self.load_history)
        self.worker.finished.connect(self.finished)
        self._update_controls()
        self._say("Łączenie z drukarką i Holuto…")
        self.worker.start()

    def _connected(self):
        self.connected = True
        self.tabs.setCurrentIndex(0)
        self._update_controls()

    def set_device(self, status, blocked):
        self.status, self.blocked = status, blocked
        record = self.printer.last_record()
        self.recovery_button.setVisible(
            bool(record and record["state"] == "pending" and record.get("operation") != "receipt")
        )
        self._update_controls()

    def stop(self):
        if self.worker is not None:
            self.worker.stop()
            self.stop_button.setEnabled(False)
            self._say("Zatrzymywanie po zakończeniu bieżącej operacji…")

    def finished(self):
        self.worker.deleteLater()
        self.worker = None
        self.connected = False
        self._update_controls()
        self.load_history()
        if self.closing:
            self.quit()

    def _submit(self, method, *args, **kwargs):
        if self.worker is not None:
            self.worker.submit(method, *args, **kwargs)

    def _confirm(self, title, text, action="Drukuj"):
        dialog = QDialog(self)
        dialog.setWindowTitle(title)
        layout = QVBoxLayout(dialog)
        body = QLabel(text)
        body.setWordWrap(True)
        layout.addWidget(body)
        layout.addLayout(dialog_footer(dialog, action))
        accepted = dialog.exec() == QDialog.DialogCode.Accepted
        dialog.deleteLater()
        return accepted

    def submit_receipt(self):
        document = self.selected_document()
        if not self.connected or not document or self.blocked:
            return
        nip = self.buyer_nip.text().strip() or None
        if nip is not None and (
            not nip.isascii() or not nip.isdigit() or len(nip) != BUYER_NIP_LENGTH
        ):
            self._say("NIP nabywcy musi zawierać 10 cyfr.", "error")
            return
        if self._confirm(
            "Potwierdź paragon",
            f"{KINDS[document['kind']]} · zlecenie {document['orderNumber']} · "
            f"{document['amount']} zł\nNIP: {nip or '—'}\n"
            "Zatwierdzenie wystawi paragon fiskalny.",
            "Dodaj do wydruku",
        ):
            self._submit(
                "queue", document["kind"], document["orderId"], document["allocationId"], nip
            )

    def submit_daily_report(self):
        if self._confirm(
            "Raport dobowy",
            "Raport dobowy zamknie bieżącą sprzedaż dnia w pamięci fiskalnej. Wydrukować raport?",
        ):
            self._submit("daily_report")

    def submit_monthly_report(self):
        year, month = self.month_picker.currentData()
        if self._confirm(
            "Raport miesięczny",
            f"Wydrukować raport miesięczny za {self.month_picker.currentText()}?",
        ):
            self._submit("monthly_report", year, month)

    def submit_periodic_report(self):
        start, end = self.period_start.date(), self.period_end.date()
        if start > end:
            self._say("Data początkowa nie może być późniejsza niż końcowa.", "error")
            return
        kind = "skrócony" if self.period_summary.isChecked() else "szczegółowy"
        if self._confirm(
            "Raport okresowy",
            f"Wydrukować raport {kind} za okres "
            f"{start.toString('dd.MM.yyyy')} – {end.toString('dd.MM.yyyy')}?",
        ):
            self._submit(
                "periodic_report",
                start.toPython(),
                end.toPython(),
                summary=self.period_summary.isChecked(),
            )

    def resolve_report(self):
        if self._confirm(
            "Wyjaśnij raport",
            "Sprawdź fizyczny wydruk i stan drukarki. Czy ustalono wynik raportu? "
            "To potwierdzenie nie ponawia wydruku.",
            "Usuń blokadę",
        ):
            self._submit("acknowledge_pending")

    def _build_tray(self):
        self.tray = QSystemTrayIcon(self.windowIcon(), self)
        self.tray.setToolTip("Holuto Fiscal")
        menu = QMenu(self)
        menu.addAction("Otwórz", self.open_window)
        menu.addAction("Zakończ", self.quit)
        self.tray.setContextMenu(menu)
        self.tray.activated.connect(self.tray_activated)
        self.tray.show()

    def open_window(self):
        self.showNormal()
        self.raise_()
        self.activateWindow()

    def tray_activated(self, reason):
        if reason in (
            QSystemTrayIcon.ActivationReason.Trigger,
            QSystemTrayIcon.ActivationReason.DoubleClick,
        ):
            self.open_window()

    def quit(self):
        self.closing = True
        if self.worker is not None:
            self.stop()
        else:
            self.tray.hide()
            self.close()
            QApplication.instance().quit()

    def closeEvent(self, event):  # noqa: N802 - Qt API override.
        if self.closing and self.worker is None:
            event.accept()
            return
        event.ignore()
        if QSystemTrayIcon.isSystemTrayAvailable():
            self.hide()
        else:
            self._say(
                "Zasobnik systemowy jest niedostępny. Aby zakończyć aplikację, kliknij Zakończ.",
                "error",
            )
