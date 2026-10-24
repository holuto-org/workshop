"""Permanent-token agent with a durable result outbox and no physical replay."""

import json
import logging
import os
import re
import tempfile
from contextlib import contextmanager
from datetime import datetime
from decimal import Decimal, InvalidOperation
from http import HTTPStatus
from importlib.metadata import version
from ipaddress import ip_address
from pathlib import Path
from typing import TYPE_CHECKING
from urllib.parse import quote, urlencode, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener
from uuid import UUID

from posnet.models import (
    EXEMPT_PERCENT,
    MAX_CENTS,
    MAX_LINES,
    MAX_NAME_LENGTH,
    PAYMENT_TYPES,
    PRINTABLE_CHARACTERS,
    Advance,
    Line,
    Payment,
)
from posnet.protocol import ProtocolError

if TYPE_CHECKING:
    from posnet.printer import Printer, Status

if os.name == "nt":
    import msvcrt

    def lock_file(file):
        msvcrt.locking(file.fileno(), msvcrt.LK_NBLCK, 1)

else:
    import fcntl

    def lock_file(file):
        fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)


log = logging.getLogger("posnet.agent")
POLL_SECONDS = 3
RETRY_SECONDS = 15
SERIAL_LIMIT = 32
TOKEN_LIMIT = 4096
VAT_RATE_LIMIT = 16
VAT_RATES = frozenset({"23", "8", "5", "0", "zw"})
DOCUMENTS_LIMIT = 100  # the largest page `/agent/documents` serves
STATES = frozenset({"queued", "taken", "done", "failed", "unknown", "expired"})
UNKNOWN_MESSAGE = "Przerwano w trakcie drukowania. Sprawdź drukarkę."


def device_serial(value: str) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > SERIAL_LIMIT
        or value != value.strip()
        or not value.isascii()
        or not value.isprintable()
    ):
        raise ValueError("Numer drukarki musi zawierać 1-32 drukowalne znaki ASCII.")
    return value


def validated_url(value: str) -> str:
    url = urlsplit(value)
    try:
        loopback = ip_address(url.hostname or "").is_loopback
    except ValueError:
        loopback = url.hostname == "localhost"
    if (
        not url.hostname
        or url.username is not None
        or url.password is not None
        or url.query
        or url.fragment
        or not (url.scheme == "https" or (url.scheme == "http" and loopback))
    ):
        raise ValueError(
            "Adres API wymaga https (http tylko dla localhost), "
            "bez danych logowania, zapytania i fragmentu."
        )
    return value.rstrip("/")


def require(
    condition, message="Nieprawidłowa odpowiedź API lub rejestr agenta. Drukowanie zablokowane."
):
    if not condition:
        raise ValueError(message)


def uuid_value(value):
    require(isinstance(value, str))
    UUID(value)
    return value


def timestamp(value):
    require(isinstance(value, str) and "T" in value)
    require(datetime.fromisoformat(value).utcoffset() is not None)
    return value


def text_fields(value, fields):
    for field in fields:
        require(isinstance(value.get(field), str))


def nullable_fields(value, fields, validate):
    for field in fields:
        require(field in value)
        if value[field] is not None:
            validate(value[field])


def amount_decimal(value):
    require(isinstance(value, str))
    try:
        number = Decimal(value)
    except InvalidOperation as exc:
        raise ValueError("Nieprawidłowa kwota w odpowiedzi API.") from exc
    require(number.is_finite() and number >= 0)
    return number


def amount_cents(value):
    number = amount_decimal(value)
    require(number <= Decimal(MAX_CENTS) / 100)
    cents = number * 100
    require(cents == cents.to_integral_value())
    return int(cents)


def document_identity(kind, order_id, allocation_id):
    require(kind in ("advance", "sale"))
    uuid_value(order_id)
    if kind == "advance":
        uuid_value(allocation_id)
    else:
        require(allocation_id is None)


def validate_payload(payload):
    require(isinstance(payload, dict))
    text_fields(payload, ("companyName", "total"))
    require(type(payload.get("orderNumber")) is int and payload["orderNumber"] > 0)
    lines = payload.get("lines")
    require(isinstance(lines, list) and 1 <= len(lines) <= MAX_LINES)
    for line in lines:
        require(isinstance(line, dict))
        text_fields(line, ("name", "quantity", "unitPrice", "total"))
        require(line.get("vatRate") in VAT_RATES)
        require(type(line.get("vatSlot")) is int and 0 <= line["vatSlot"] < VAT_RATE_LIMIT)
    require(payload.get("kind") in ("advance", "sale"))
    advances = payload.get("advances")
    require(isinstance(advances, list))
    require(payload["kind"] != "advance" or not advances)
    for item in advances:
        require(isinstance(item, dict))
        text_fields(item, ("amount",))
        require(item.get("vatRate") in VAT_RATES)
        require(type(item.get("vatSlot")) is int and 0 <= item["vatSlot"] < VAT_RATE_LIMIT)
    payments = payload.get("payments")
    require(isinstance(payments, list))
    for item in payments:
        require(isinstance(item, dict))
        text_fields(item, ("amount", "form"))
        require(item["form"] in PAYMENT_TYPES)


def validate_job(job, printer_id=None):
    require(isinstance(job, dict))
    for field in ("id", "orderId", "printerId"):
        uuid_value(job.get(field))
    require(printer_id is None or job["printerId"] == printer_id)
    require(job.get("state") in STATES)
    nullable_fields(
        job,
        ("buyerNip",),
        lambda value: require(
            isinstance(value, str) and re.fullmatch(r"[0-9]{10}", value) is not None
        ),
    )
    validate_payload(job)
    nullable_fields(job, ("attempt",), uuid_value)
    nullable_fields(job, ("receiptNumber", "result"), lambda value: require(isinstance(value, str)))
    timestamp(job.get("createdAt"))
    nullable_fields(job, ("takenAt", "resolvedAt"), timestamp)
    return job


def validate_result(body):
    require(isinstance(body, dict))
    require(set(body) == {"attempt", "state", "receiptNumber", "issuedAt", "result"})
    uuid_value(body["attempt"])
    require(body["state"] in ("done", "failed", "unknown"))
    nullable_fields(
        body, ("receiptNumber", "result"), lambda value: require(isinstance(value, str))
    )
    if body["state"] == "done":
        timestamp(body["issuedAt"])
    else:
        require(body["issuedAt"] is None and body["receiptNumber"] is None)


def validate_printer(status, serial):
    require(isinstance(status, dict))
    require(type(status.get("isOnline")) is bool)
    nullable_fields(status, ("lastSeenAt",), timestamp)
    timestamp(status.get("updatedAt"))
    printer = status.get("printer")
    require(isinstance(printer, dict))
    uuid_value(printer.get("id"))
    uuid_value(printer.get("branchId"))
    require(printer.get("deviceSerial") in (None, serial))
    for field in ("model", "firmware", "appVersion"):
        require(printer.get(field) is None or isinstance(printer[field], str))
    nullable_fields(
        printer,
        ("maxItemNameLength", "maxLinesPerReceipt"),
        lambda value: require(type(value) is int and value > 0),
    )
    nullable_fields(
        printer, ("allowedCharacters",), lambda value: require(isinstance(value, str) and value)
    )
    slots = printer.get("vatSlots")
    require(isinstance(slots, list) and len(slots) <= VAT_RATE_LIMIT)
    for slot in slots:
        require(isinstance(slot, dict) and set(slot) == {"slot", "vatRate"})
        require(type(slot["slot"]) is int and 0 <= slot["slot"] < VAT_RATE_LIMIT)
        require(slot["vatRate"] in VAT_RATES)
    return printer


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        # urllib normally forwards Authorization to the redirect target.
        return None


class Api:
    def __init__(self, base_url: str, token: str, serial: str):
        self.base_url = validated_url(base_url)
        require(
            isinstance(token, str)
            and 1 <= len(token) <= TOKEN_LIMIT
            and token.isascii()
            and token.isprintable()
        )
        require(not any(character.isspace() for character in token))
        self.token = token
        self.serial = device_serial(serial)
        self.printer_id = None
        self.opener = build_opener(NoRedirect())
        # The posnet driver's Temo Online limits: the protocol cannot report them, and the
        # backend refuses to queue receipts for a printer that never sent them.
        self.telemetry = {
            "serial": self.serial,
            "appVersion": version("fiscal-desktop"),
            "model": "",
            "firmware": "",
            "vatSlots": [],
            "maxItemNameLength": MAX_NAME_LENGTH,
            "maxLinesPerReceipt": MAX_LINES,
            "allowedCharacters": PRINTABLE_CHARACTERS,
        }

    def describe(self, status: Status) -> None:
        if device_serial(status.unique_number) != self.serial:
            raise ValueError("Podłącz drukarkę sparowaną przy uruchomieniu agenta.")
        slots = []
        for vat in status.vat_rates:
            if not vat.active:
                continue
            rate = "zw" if vat.percent == EXEMPT_PERCENT else f"{vat.percent.normalize():f}"
            if rate not in VAT_RATES:
                log.warning("Pomijam nieobsługiwaną stawkę VAT w slocie %s: %s", vat.index, rate)
                continue
            slots.append({"slot": vat.index, "vatRate": rate})
        self.telemetry.update(model=status.name, firmware=status.version, vatSlots=slots)

    def _send(self, method: str, path: str, body: dict | None = None):
        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self.token}",
            "App-Version": f"agent/{self.telemetry['appVersion']}",
        }
        if path not in ("session", "heartbeat"):
            headers["X-Device-Serial"] = self.serial
        request = Request(  # noqa: S310 - validated HTTPS or loopback HTTP; redirects disabled
            f"{self.base_url}/agent/{path}",
            data=json.dumps(body).encode() if body is not None else None,
            headers=headers,
            method=method,
        )
        with self.opener.open(request, timeout=30) as response:
            if response.status == HTTPStatus.NO_CONTENT:
                require(path == "jobs/take")
                return None
            require(response.status in (HTTPStatus.OK, HTTPStatus.CREATED))
            return json.load(response)

    def _telemetry(self, path):
        printer = validate_printer(self._send("POST", path, self.telemetry), self.serial)
        require(self.printer_id is None or self.printer_id == printer["id"])
        self.printer_id = printer["id"]
        return printer

    def connect(self):
        return self._telemetry("session")

    def heartbeat(self):
        return self._telemetry("heartbeat")

    def take(self) -> dict | None:
        job = self._send("POST", "jobs/take")
        if job is not None:
            validate_job(job, self.printer_id)
            require(job["state"] == "taken" and job["attempt"] is not None)
        return job

    def status(self, job_id: str) -> dict:
        uuid_value(job_id)
        job = validate_job(self._send("GET", f"jobs/{job_id}"), self.printer_id)
        require(job["id"] == job_id)
        return job

    def report(self, job_id: str, body: dict):
        uuid_value(job_id)
        validate_result(body)
        require(body["state"] != "done" or bool(body["receiptNumber"]))
        job = validate_job(self._send("POST", f"jobs/{job_id}/result", body), self.printer_id)
        require(job["id"] == job_id and job["attempt"] == body["attempt"])
        require(job["state"] == body["state"])
        require(job["receiptNumber"] == body["receiptNumber"] and job["result"] == body["result"])
        return job

    def documents(self, cursor: str | None = None) -> dict:
        """One page of printable documents, oldest first.

        Pass the page's `nextCursor` back unchanged for the following page; null ends the list.
        Every item is validated before the page is returned, so a bad page yields nothing.
        """
        require(cursor is None or (isinstance(cursor, str) and bool(cursor)))
        query = {"limit": DOCUMENTS_LIMIT} | ({} if cursor is None else {"cursor": cursor})
        response = self._send("GET", f"documents?{urlencode(query, quote_via=quote)}")
        require(isinstance(response, dict) and isinstance(response.get("items"), list))
        require(len(response["items"]) <= DOCUMENTS_LIMIT and "nextCursor" in response)
        next_cursor = response["nextCursor"]
        require(next_cursor is None or (isinstance(next_cursor, str) and bool(next_cursor)))
        for item in response["items"]:
            require(isinstance(item, dict) and "allocationId" in item)
            document_identity(item.get("kind"), item.get("orderId"), item["allocationId"])
            amount_decimal(item.get("amount"))
            require(type(item.get("orderNumber")) is int and item["orderNumber"] > 0)
            timestamp(item.get("occurredAt"))
            nullable_fields(item, ("customerName",), lambda value: require(isinstance(value, str)))
        return {"items": response["items"], "nextCursor": next_cursor}

    def queue(self, kind, order_id, allocation_id, buyer_nip=None):
        document_identity(kind, order_id, allocation_id)
        body = {
            "kind": kind,
            "orderId": order_id,
            "allocationId": allocation_id,
            "buyerNip": buyer_nip,
        }
        return validate_job(self._send("POST", "jobs", body), self.printer_id)


def durable_save(path: Path, record: dict):
    fd, name = tempfile.mkstemp(dir=path.parent, prefix=".agent-")
    temporary = Path(name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as file:
            json.dump(record, file, ensure_ascii=False)
            file.flush()
            os.fsync(file.fileno())
        temporary.replace(path)
        if hasattr(os, "O_DIRECTORY"):
            directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


class Agent:
    def __init__(self, printer: Printer, api: Api):
        self.printer = printer
        self.api = api
        self.outbox_path = printer.journal_path.with_suffix(".agent.json")

    @contextmanager
    def _locked(self):
        with self.outbox_path.with_suffix(".lock").open("ab") as lock:
            try:
                lock_file(lock)
            except OSError as exc:
                raise ValueError("Inny agent używa tego rejestru operacji.") from exc
            yield

    @property
    def blocked(self):
        outbox = self._load_outbox()
        return bool(self.printer.pending() or (outbox and not outbox["settled"]))

    def run_report(self, method, *args, **kwargs):
        require(
            method in ("daily_report", "monthly_report", "periodic_report", "acknowledge_pending")
        )
        with self._locked():
            record = self.printer.last_record()
            if method == "acknowledge_pending":
                if not record or record.get("operation") == "receipt":
                    raise ValueError("Wynik paragonu rozstrzyga personel w aplikacji Holuto.")
            elif self._settle_last_operation():
                raise ValueError("Najpierw rozstrzygnij niepotwierdzony paragon w Holuto.")
            result = getattr(self.printer, method)(*args, **kwargs)
            if result and result["state"] == "completed":
                self.printer.acknowledge(result)

    def step(self):
        with self._locked():
            probe_error = None
            try:
                status = self.printer.probe()
                self.api.describe(status)
            except (OSError, ProtocolError, ValueError) as exc:
                probe_error = exc
            # An unready, disconnected or blocked device still updates its last-seen time.
            self.api.heartbeat()
            if self._settle_last_operation():
                log.warning(
                    "Niepotwierdzony wynik. Sprawdź wydruk i rozstrzygnij zadanie w Holuto."
                )
                return RETRY_SECONDS
            if probe_error is not None:
                raise probe_error
            if not status.ready:
                log.warning("Drukarka niegotowa: %s", status.description)
                return RETRY_SECONDS
            job = self.api.take()
            if job is None:
                return POLL_SECONDS
            self._execute(job)
            return 0

    def _load_outbox(self):
        try:
            outbox = json.loads(self.outbox_path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return None
        require(isinstance(outbox, dict))
        uuid_value(outbox.get("jobId"))
        uuid_value(outbox.get("attempt"))
        timestamp(outbox.get("issuedAt"))
        require(type(outbox.get("settled")) is bool and "body" in outbox)
        if outbox["body"] is not None:
            validate_result(outbox["body"])
            require(outbox["body"]["attempt"] == outbox["attempt"])
            if outbox["body"]["state"] == "done":
                require(outbox["body"]["issuedAt"] == outbox["issuedAt"])
        if outbox["settled"] and outbox["body"] is None:
            require(outbox.get("resolvedState") in ("done", "failed", "expired"))
        return outbox

    def _matching_record(self, outbox):
        record = self.printer.last_record()
        if record and record.get("job_id") == outbox["jobId"]:
            require(record.get("attempt") == outbox["attempt"])
            require(record.get("issued_at") == outbox["issuedAt"])
            require(record.get("unique_number") == self.api.serial)
            return record
        return None

    def _finish(self, outbox, record):
        if record and record["state"] == "completed" and record.get("operation") == "receipt":
            try:
                self._save_history(record)
            except (OSError, ValueError) as exc:
                log.warning("Nie można zapisać historii: %s", exc)
        if record and record["state"] == "pending":
            self.printer.acknowledge_pending()
        elif record and record["state"] != "acknowledged":
            self.printer.acknowledge(record)
        outbox["settled"] = True
        durable_save(self.outbox_path, outbox)

    def _save_history(self, record):
        path = self.printer.journal_path.with_name("history.jsonl")
        try:
            rows = [
                json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line
            ]
        except FileNotFoundError:
            rows = []
        if not any(row.get("job_id") == record.get("job_id") for row in rows):
            with path.open("a", encoding="utf-8") as file:
                file.write(json.dumps(record, ensure_ascii=False) + "\n")
                file.flush()
                os.fsync(file.fileno())

    def _settle_last_operation(self) -> bool:
        outbox = self._load_outbox()
        if outbox is None or outbox["settled"]:
            record = self.printer.last_record()
            if (
                record
                and record["state"] == "completed"
                and record.get("operation") in ("daily_report", "monthly_report", "periodic_report")
            ):
                self.printer.acknowledge(record)
                return False
            require(not record or record["state"] == "acknowledged")
            return False
        record = self._matching_record(outbox)
        remote = validate_job(self.api.status(outbox["jobId"]), self.api.printer_id)
        require(remote["id"] == outbox["jobId"] and remote["attempt"] == outbox["attempt"])
        if remote["state"] in ("done", "failed", "expired"):
            outbox["resolvedState"] = remote["state"]
            self._finish(outbox, record)
            return False
        body = outbox["body"]
        if remote["state"] == "unknown" and body and body["state"] == "failed":
            # Even a known preflight failure cannot automatically clear a server UNKNOWN.
            return True
        if (
            record
            and record["state"] == "completed"
            and (
                body is None
                or (body["state"] == "unknown" and record.get("receipt_number"))
                or (body["state"] == "done" and not body["receiptNumber"])
            )
        ):
            self._report(outbox, "done", record=record)
        elif body is None:
            self._report(outbox, "unknown", result=UNKNOWN_MESSAGE)
        elif body["state"] != "unknown" or remote["state"] != "unknown":
            self.api.report(outbox["jobId"], body)
        if outbox["body"]["state"] == "unknown":
            return True
        self._finish(outbox, record)
        return False

    def _report(self, outbox, state, *, record=None, result=None):
        if state == "done" and not record.get("receipt_number"):
            state = "unknown"
            result = (
                "Wydruk zakończony, ale nie odczytano numeru paragonu. "
                "Sprawdź wydruk i rozstrzygnij zadanie w Holuto."
            )
        body = {
            "attempt": outbox["attempt"],
            "state": state,
            "receiptNumber": record["receipt_number"] if state == "done" else None,
            "issuedAt": outbox["issuedAt"] if state == "done" else None,
            "result": result[:2000] if result else None,
        }
        validate_result(body)
        outbox["body"] = body
        durable_save(self.outbox_path, outbox)
        self.api.report(outbox["jobId"], body)

    def _execute(self, job):
        validate_job(job, self.api.printer_id)
        require(job["state"] == "taken" and job["attempt"] is not None)
        previous = self._load_outbox()
        require(previous is None or (previous["settled"] and previous["jobId"] != job["id"]))
        outbox = {
            "jobId": job["id"],
            "attempt": job["attempt"],
            "issuedAt": datetime.now().astimezone().isoformat(),
            "body": None,
            "settled": False,
        }
        # Persist the claim before any possible physical action; an interrupted dispatch is UNKNOWN.
        durable_save(self.outbox_path, outbox)
        self.api.describe(self.printer.probe())
        try:
            rates = {slot["slot"]: slot["vatRate"] for slot in self.api.telemetry["vatSlots"]}
            require(
                all(
                    rates.get(item["vatSlot"]) == item["vatRate"]
                    for item in [*job["lines"], *job["advances"]]
                )
            )
            lines = [
                Line(
                    item["name"],
                    Decimal(item["quantity"]),
                    Decimal(item["unitPrice"]),
                    item["vatSlot"],
                )
                for item in job["lines"]
            ]
            for line, item in zip(lines, job["lines"], strict=True):
                require(
                    line.total_cents == amount_cents(item["total"]),
                    "Wartość pozycji nie zgadza się z zaokrąglonym iloczynem ilości i ceny.",
                )
            require(sum(line.total_cents for line in lines) == amount_cents(job["total"]))
            advances = [
                Advance(item["vatSlot"], amount_cents(item["amount"])) for item in job["advances"]
            ]
            payments = [
                Payment(item["form"], amount_cents(item["amount"])) for item in job["payments"]
            ]
            record = self.printer.print_receipt(
                lines,
                payments,
                advances=advances,
                buyer_nip=job["buyerNip"],
                brand=job["companyName"],
                job_id=job["id"],
                attempt=job["attempt"],
                issued_at=outbox["issuedAt"],
            )
        except (OSError, ProtocolError, ValueError, InvalidOperation) as exc:
            record = self._matching_record(outbox)
            state = "unknown" if record else "failed"
            self._report(outbox, state, result=str(exc))
            if state == "failed":
                self._finish(outbox, None)
            return
        self._report(outbox, "done", record=record)
        if outbox["body"]["state"] == "unknown":
            return
        self._finish(outbox, record)
        log.info("Paragon %s dla zlecenia (job %s)", record.get("receipt_number"), job["id"])


def load_config(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except OSError, ValueError:
        return {}


def save_config(path: Path, config: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    durable_save(path, config)
    path.chmod(0o600)
