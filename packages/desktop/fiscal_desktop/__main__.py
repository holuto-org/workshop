import argparse
import json
import logging
import sys
from dataclasses import asdict, replace
from pathlib import Path

from posnet.printer import Printer
from PySide6.QtCore import QStandardPaths


def state_directory(state):
    data = state / "holuto-fiscal"
    existing = [
        path
        for path in (data, state / "workshop-agent", state / "huggingcar-fiscal")
        if path.is_dir()
    ]
    if len(existing) > 1:
        raise ValueError(
            "Istnieje kilka rejestrów fiskalnych. Uzgodnij ich stan przed uruchomieniem; "
            "żadnego nie usuwaj."
        )
    if existing and existing[0] != data:
        existing[0].rename(data)
    return data


def configured_printer(args):
    from .app import migrate_legacy_settings, saved_connection

    data = args.data_dir
    if data is None:
        state = Path(
            QStandardPaths.writableLocation(QStandardPaths.StandardLocation.GenericStateLocation)
        )
        data = state_directory(state)
    migrate_legacy_settings()
    connection = saved_connection(data)
    if args.serial is not None:
        connection = replace(connection, address=args.serial)
    if args.baudrate is not None:
        connection = replace(connection, baudrate=args.baudrate)
    journal = data / "operation.json"
    agent_journal = data / "fiscal-operation.json"
    if agent_journal.exists():
        if journal.exists():
            raise ValueError(
                "Istnieją dwa rejestry operacji. Uzgodnij ich stan; żadnego nie usuwaj."
            )
        journal = agent_journal
    return Printer(connection, journal)


def main():
    parser = argparse.ArgumentParser(description="Holuto Fiscal · Posnet Temo Online")
    parser.add_argument(
        "--probe", action="store_true", help="Read printer identity, status and VAT, then exit"
    )
    parser.add_argument("--serial", help="USB serial device, e.g. /dev/serial/by-id/...")
    parser.add_argument("--baudrate", type=int, help="Override the saved baudrate (default 9600)")
    parser.add_argument("--data-dir", type=Path, help="Override application state directory")
    args = parser.parse_args()
    if args.probe:
        try:
            status = configured_printer(args).probe()
        except (OSError, ValueError, RuntimeError) as exc:
            print(str(exc), file=sys.stderr)
            return 1
        print(json.dumps(asdict(status), ensure_ascii=False, indent=2, default=str))
        return 0 if status.ready else 1
    from PySide6.QtCore import QLocale
    from PySide6.QtGui import QIcon
    from PySide6.QtWidgets import QApplication, QMessageBox

    from .app import MainWindow

    QLocale.setDefault(QLocale("pl_PL"))
    app = QApplication(sys.argv[:1])
    app.setStyle("Fusion")
    app.setQuitOnLastWindowClosed(False)
    app.setApplicationName("Holuto Fiscal")
    app.setOrganizationName("Holuto")
    app.setDesktopFileName("holuto-fiscal")
    app.setWindowIcon(QIcon(str(Path(__file__).with_name("icon.svg"))))
    logging.getLogger("posnet.agent").setLevel(logging.INFO)
    try:
        window = MainWindow(configured_printer(args))
    except (OSError, ValueError) as exc:
        QMessageBox.critical(None, "Holuto Fiscal", f"Nie można uruchomić aplikacji:\n{exc}")
        return 1
    window.show()
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
