"""Permanent-token agent with a durable result outbox and no physical replay."""

import json
import logging
import os
import re
import tempfile
import time
from contextlib import contextmanager
from datetime import datetime
from decimal import Decimal, InvalidOperation
from http import HTTPStatus
from importlib.metadata import version
from ipaddress import ip_address
from pathlib import Path
from typing import TYPE_CHECKING
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener
from uuid import UUID

from posnet.models import EXEMPT_PERCENT, Line
from posnet.printer import MAX_LINES
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


def require(condition):
    if not condition:
        raise ValueError("Nieprawidłowa odpowiedź API lub rejestr agenta. Drukowanie zablokowane.")


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


def validate_payload(payload):
    require(isinstance(payload, dict))
    text_fields(payload, ("companyName",))
    lines = payload.get("lines")
    require(isinstance(lines, list) and 1 <= len(lines) <= MAX_LINES)
    for line in lines:
        require(isinstance(line, dict))
        text_fields(line, ("name", "quantity", "unitPrice"))


def validate_job(job, printer_id=None):
    require(isinstance(job, dict))
    for field in ("id", "orderId", "printerId"):
        uuid_value(job.get(field))
    require(printer_id is None or job["printerId"] == printer_id)
    require(job.get("state") in STATES)
    require(job.get("payment") in ("cash", "card"))
    validate_payload(job.get("payload"))
    nullable_fields(job, ("attempt",), uuid_value)
    nullable_fields(job, ("receiptNumber", "result"), lambda value: require(isinstance(value, str)))
    timestamp(job.get("createdAt"))
    nullable_fields(job, ("takenAt", "resolvedAt"), timestamp)
    text_fields(job, ("etag",))
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


def validate_printer(printer, serial):
    require(isinstance(printer, dict))
    uuid_value(printer.get("id"))
    uuid_value(printer.get("branchId"))
    require(printer.get("deviceSerial") == serial)
    text_fields(printer, ("name", "vendor", "deviceModel", "firmware", "agentVersion", "etag"))
    require(type(printer.get("isOnline")) is bool)
    nullable_fields(printer, ("lastSeenAt",), timestamp)
    rates = printer.get("vatRates")
    require(isinstance(rates, list) and len(rates) <= VAT_RATE_LIMIT)
    for rate in rates:
        require(isinstance(rate, dict) and set(rate) == {"index", "percent"})
        require(type(rate["index"]) is int and 0 <= rate["index"] < VAT_RATE_LIMIT)
        require(isinstance(rate["percent"], str))
        require(re.fullmatch(r"(?:[0-9]{1,2}(?:\.[0-9]{1,2})?|zw)", rate["percent"]) is not None)
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
        self.telemetry = {
            "serial": self.serial,
            "vendor": "posnet",
            "model": "",
            "firmware": "",
            "agentVersion": version("workshop-agent"),
            "vatRates": [],
        }

    def describe(self, status: Status) -> None:
        if device_serial(status.unique_number) != self.serial:
            raise ValueError("Podłącz drukarkę sparowaną przy uruchomieniu agenta.")
        self.telemetry.update(
            model=status.name,
            firmware=status.version,
            vatRates=[
                {
                    "index": vat.index,
                    "percent": "zw"
                    if vat.percent == EXEMPT_PERCENT
                    else f"{vat.percent.normalize():f}",
                }
                for vat in status.vat_rates
                if vat.active
            ],
        )

    def _send(self, method: str, path: str, body: dict | None = None):
        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {self.token}",
            "App-Version": f"agent/{self.telemetry['agentVersion']}",
        }
        if path != "session":
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
            require(response.status == HTTPStatus.OK)
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
        job = validate_job(self._send("POST", f"jobs/{job_id}/result", body), self.printer_id)
        require(job["id"] == job_id and job["attempt"] == body["attempt"])
        require(job["state"] == body["state"])
        require(job["receiptNumber"] == body["receiptNumber"] and job["result"] == body["result"])
        return job


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
        self.stopping = False
        self.outbox_path = printer.journal_path.with_suffix(".agent.json")

    @contextmanager
    def _locked(self):
        with self.outbox_path.with_suffix(".lock").open("ab") as lock:
            try:
                lock_file(lock)
            except OSError as exc:
                raise ValueError("Inny agent używa tego rejestru operacji.") from exc
            yield

    def run_forever(self):
        # A stop request waits for the current receipt; device commands are never replayed.
        while not self.stopping:
            try:
                self.step()
            except HTTPError as exc:
                if exc.code == HTTPStatus.UNAUTHORIZED:
                    raise
                log.warning("%s — ponowna próba za %ss", exc, RETRY_SECONDS)
                time.sleep(RETRY_SECONDS)
            except (
                OSError,
                ProtocolError,
                ValueError,
                LookupError,
                TypeError,
                InvalidOperation,
            ) as exc:
                log.warning("%s — ponowna próba za %ss", exc, RETRY_SECONDS)
                time.sleep(RETRY_SECONDS)
        log.info("Zatrzymano")

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
                time.sleep(RETRY_SECONDS)
                return
            if probe_error is not None:
                raise probe_error
            if not status.ready:
                log.warning("Drukarka niegotowa: %s", status.description)
                time.sleep(RETRY_SECONDS)
                return
            job = self.api.take()
            if job is None:
                time.sleep(POLL_SECONDS)
                return
            self._execute(job)

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
        if record and record["state"] == "pending":
            self.printer.acknowledge_pending()
        elif record and record["state"] != "acknowledged":
            self.printer.acknowledge(record)
        outbox["settled"] = True
        durable_save(self.outbox_path, outbox)

    def _settle_last_operation(self) -> bool:
        outbox = self._load_outbox()
        if outbox is None or outbox["settled"]:
            record = self.printer.last_record()
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
            and (body is None or body["state"] == "unknown")
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
        body = {
            "attempt": outbox["attempt"],
            "state": state,
            "receiptNumber": (record.get("receipt_number") or None) if record else None,
            "issuedAt": outbox["issuedAt"] if state == "done" else None,
            "result": result[:2000] if result else None,
        }
        validate_result(body)
        outbox["body"] = body
        durable_save(self.outbox_path, outbox)
        self.api.report(outbox["jobId"], body)

    def _standard_vat_slot(self) -> int:
        # Automotive receipts use one device-programmed rate, usually PL 23%.
        status = self.printer.last_status
        active = [vat for vat in (status.vat_rates if status else ()) if vat.active]
        taxed = [vat for vat in active if vat.percent != EXEMPT_PERCENT]
        if taxed:
            return max(taxed, key=lambda vat: vat.percent).index
        if active:
            return active[0].index
        raise ValueError("Drukarka nie ma zaprogramowanej żadnej stawki VAT.")

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
            vat = self._standard_vat_slot()
            lines = [
                Line(item["name"], Decimal(item["quantity"]), Decimal(item["unitPrice"]), vat)
                for item in job["payload"]["lines"]
            ]
            record = self.printer.print_receipt(
                lines,
                0 if job["payment"] == "cash" else 2,
                brand=job["payload"]["companyName"],
                job_id=job["id"],
                attempt=job["attempt"],
                issued_at=outbox["issuedAt"],
            )
        except (ValueError, InvalidOperation) as exc:
            self._report(outbox, "failed", result=str(exc))
            self._finish(outbox, None)
            return
        except (OSError, ProtocolError) as exc:
            record = self._matching_record(outbox)
            state = "unknown" if record else "failed"
            self._report(outbox, state, result=str(exc))
            if state == "failed":
                self._finish(outbox, None)
            return
        self._report(outbox, "done", record=record)
        self._finish(outbox, record)
        log.info("Paragon %s dla zlecenia (job %s)", record.get("receipt_number"), job["id"])


def load_config(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except OSError, ValueError:
        return {}


def save_config(path: Path, config: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch(mode=0o600)
    path.chmod(0o600)
    path.write_text(json.dumps(config, indent=2))
