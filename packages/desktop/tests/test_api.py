"""Real HTTP exchanges for the permanent-token, UUID-attempt protocol."""

import copy
import json
import re
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlsplit

import pytest
from fiscal_desktop.agent import Agent, Api
from posnet.models import PRINTABLE_CHARACTERS
from posnet.printer import Printer
from posnet.simulator import Simulator
from test_agent import ADVANCE_JOB, ATTEMPT, JOB, JOB_ID, PRINTER, STATUS, TELEMETRY_FIELDS


@contextmanager
def server(replies):
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            requests.append((self.path, self.headers, json.loads(body) if body else None))
            code, payload, headers = replies.pop(0)
            self.send_response(code)
            for key, value in headers.items():
                self.send_header(key, value)
            self.end_headers()
            self.wfile.write(json.dumps(payload).encode())

        def do_GET(self):
            self.do_POST()

        def log_message(self, *_args):
            pass

    httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=httpd.serve_forever)
    thread.start()
    try:
        yield f"http://127.0.0.1:{httpd.server_port}", requests
    finally:
        httpd.shutdown()
        httpd.server_close()
        thread.join()


def session():
    return (200, copy.deepcopy(STATUS), {})


def test_session_heartbeat_take_status_result_use_permanent_bearer_and_clean_paths():
    body = {
        "attempt": ATTEMPT,
        "state": "done",
        "receiptNumber": "1",
        "issuedAt": "2026-10-16T10:02:00Z",
        "result": None,
    }
    done = {**JOB, "state": "done", "receiptNumber": "1", "resolvedAt": body["issuedAt"]}
    with server(
        [session(), session(), (200, JOB, {}), (200, JOB, {}), (200, done, {}), (204, None, {})]
    ) as (url, requests):
        api = Api(url, "secret", "DEMO0000001")
        api.connect()
        api.heartbeat()
        assert api.take() == JOB
        assert api.status(JOB_ID) == JOB
        api.report(JOB_ID, body)
        assert api.take() is None
    assert [headers["Authorization"] for _, headers, _ in requests] == ["Bearer secret"] * 6
    # The backend's App-Version gate parses SemVer; PEP 440 forms like 1.0rc1 would be rejected.
    assert re.fullmatch(r"\d+\.\d+\.\d+", api.telemetry["appVersion"])
    assert all(
        headers["App-Version"] == f"agent/{api.telemetry['appVersion']}"
        for _, headers, _ in requests
    )
    assert [path for path, _, _ in requests] == [
        "/agent/session",
        "/agent/heartbeat",
        "/agent/jobs/take",
        f"/agent/jobs/{JOB_ID}",
        f"/agent/jobs/{JOB_ID}/result",
        "/agent/jobs/take",
    ]
    assert all(headers.get("X-Device-Serial") is None for _, headers, _ in requests[:2])
    assert all(headers["X-Device-Serial"] == "DEMO0000001" for _, headers, _ in requests[2:])
    assert requests[0][2] == requests[1][2] == api.telemetry
    for _, _, telemetry in requests[:2]:  # session and heartbeat both carry the capabilities
        assert set(telemetry) == TELEMETRY_FIELDS
        assert telemetry["maxItemNameLength"] == 80
        assert telemetry["maxLinesPerReceipt"] == 500
        assert telemetry["allowedCharacters"] == PRINTABLE_CHARACTERS
    assert requests[2][2] is None
    assert requests[3][2] is None
    assert requests[4][2] == body


@pytest.mark.parametrize("operation", ["connect", "heartbeat", "take", "status", "report"])
def test_401_fails_without_refresh_or_retry(operation):
    with server([(401, {}, {})]) as (url, requests):
        api = Api(url, "rotated-secret", "DEMO0000001")
        api.printer_id = PRINTER["id"]
        args = {
            "status": (JOB_ID,),
            "report": (
                JOB_ID,
                {
                    "attempt": ATTEMPT,
                    "state": "unknown",
                    "receiptNumber": None,
                    "issuedAt": None,
                    "result": "uncertain",
                },
            ),
        }.get(operation, ())
        with pytest.raises(HTTPError) as exc:
            getattr(api, operation)(*args)
        assert exc.value.code == 401
    assert len(requests) == 1
    assert requests[0][1]["Authorization"] == "Bearer rotated-secret"


@pytest.mark.parametrize("code", [302, 500])
def test_redirects_and_server_errors_do_not_replay(code):
    with server([(code, {}, {"Location": "/steal-credential"})]) as (url, requests):
        with pytest.raises(HTTPError) as exc:
            Api(url, "secret", "DEMO0000001").take()
        assert exc.value.code == code
    assert len(requests) == 1


@pytest.mark.parametrize(
    "payload",
    [
        None,
        {},
        PRINTER,
        {**STATUS, "printer": {**PRINTER, "deviceSerial": "OTHER"}},
        {**STATUS, "isOnline": 1},
        {**STATUS, "printer": {**PRINTER, "id": 7}},
        {**STATUS, "printer": {**PRINTER, "vatSlots": [{"slot": True, "vatRate": "23"}]}},
        {**STATUS, "printer": {**PRINTER, "maxItemNameLength": 0}},
        {**STATUS, "printer": {**PRINTER, "maxLinesPerReceipt": "500"}},
        {**STATUS, "printer": {**PRINTER, "allowedCharacters": ""}},
        {
            **STATUS,
            "printer": {k: v for k, v in PRINTER.items() if k != "allowedCharacters"},
        },
    ],
)
def test_malformed_or_wrong_printer_session_is_rejected(payload):
    with server([(200, payload, {})]) as (url, _requests), pytest.raises(ValueError):
        Api(url, "secret", "DEMO0000001").connect()


@pytest.mark.parametrize(
    "payload",
    [
        {},
        {**JOB, "attempt": None},
        {**JOB, "state": "unknown"},
        {
            **JOB,
            "lines": [
                {"name": "x", "quantity": 1, "unitPrice": "10.00", "vatRate": "23", "vatSlot": 0}
            ],
        },
    ],
)
def test_malformed_take_response_is_rejected_before_execution(payload):
    with server([(200, payload, {})]) as (url, _requests):
        api = Api(url, "secret", "DEMO0000001")
        api.printer_id = PRINTER["id"]
        with pytest.raises(ValueError):
            api.take()


def test_status_rejects_wrong_job_and_result_rejects_wrong_attempt():
    body = {
        "attempt": ATTEMPT,
        "state": "unknown",
        "receiptNumber": None,
        "issuedAt": None,
        "result": None,
    }
    with server(
        [
            (200, {**JOB, "id": PRINTER["id"]}, {}),
            (200, {**JOB, "state": "unknown", "attempt": PRINTER["id"]}, {}),
        ]
    ) as (url, _requests):
        api = Api(url, "secret", "DEMO0000001")
        api.printer_id = PRINTER["id"]
        with pytest.raises(ValueError):
            api.status(JOB_ID)
        with pytest.raises(ValueError):
            api.report(JOB_ID, body)


@pytest.mark.parametrize(
    "url",
    [
        "ftp://server",
        "http://server",
        "https://user:pass@server",
        "https://server?token=secret",
        "https://server/#fragment",
        "https:///missing-host",
    ],
)
def test_unsafe_api_urls_are_rejected(url):
    with pytest.raises(ValueError):
        Api(url, "secret", "DEMO0000001")


@pytest.mark.parametrize("serial", ["", " ABC", "ABC ", "A\r\nB", "A\x00B", "Żółw", "9" * 33])
def test_invalid_serial_is_rejected_not_encoded_or_truncated(serial):
    with pytest.raises(ValueError):
        Api("https://api.example", "secret", serial)


def test_serial_is_sent_exactly_without_percent_encoding():
    serial = "AB/1 %2"
    with server([(204, None, {})]) as (url, requests):
        Api(url, "secret", serial).take()
    assert requests[0][1]["X-Device-Serial"] == serial


def test_invalid_result_response_preserves_journal_and_retries_identical_body(tmp_path):
    done = {**JOB, "state": "done", "receiptNumber": "1", "resolvedAt": JOB["takenAt"]}
    replies = [
        session(),
        session(),
        (200, JOB, {}),
        (200, {}, {}),
        session(),
        (200, JOB, {}),
        (200, done, {}),
        (204, None, {}),
    ]
    with Simulator() as sim, server(replies) as (url, requests):
        path = tmp_path / "operation.json"
        printer = Printer(sim.connection, path)
        api = Api(url, "secret", "DEMO0000001")
        api.describe(printer.probe())
        api.connect()
        with pytest.raises(ValueError):
            Agent(printer, api).step()
        assert printer.last_record()["state"] == "completed"
        Agent(Printer(sim.connection, path), api).step()
        results = [body for request_path, _, body in requests if request_path.endswith("/result")]
        assert len(results) == 2
        assert results[0] == results[1]
        assert printer.last_record()["state"] == "acknowledged"
        assert len(sim.receipts) == 1


def test_rotated_credential_stops_running_loop_without_retry(tmp_path):
    with Simulator() as sim, server([(401, {}, {})]) as (url, requests):
        printer = Printer(sim.connection, tmp_path / "operation.json")
        api = Api(url, "old-credential", "DEMO0000001")
        with pytest.raises(HTTPError) as exc:
            Agent(printer, api).step()
        assert exc.value.code == 401
        assert len(requests) == 1
        assert requests[0][0] == "/agent/heartbeat"
        assert sim.receipts == []


@pytest.mark.parametrize("kind", ["advance", "sale"])
def test_documents_and_queue_use_contract_and_device_serial(kind):
    order_id = JOB["orderId"]
    allocation_id = ADVANCE_JOB["allocationId"] if kind == "advance" else None
    documents = {
        "items": [
            {
                "kind": kind,
                "orderId": order_id,
                "orderNumber": 10,
                "allocationId": allocation_id,
                "amount": "250.00",
                "customerName": None,
                "occurredAt": JOB["createdAt"],
            }
        ],
        "nextCursor": None,
    }
    queued = {
        **(ADVANCE_JOB if kind == "advance" else JOB),
        "state": "queued",
        "attempt": None,
        "takenAt": None,
        "buyerNip": "5260250274",
    }
    with server([(200, documents, {}), (201, queued, {})]) as (url, requests):
        api = Api(url, "secret", "DEMO0000001")
        api.printer_id = PRINTER["id"]
        assert api.documents() == documents
        assert api.queue(kind, order_id, allocation_id, "5260250274") == queued
    assert requests[0][0] == "/agent/documents?limit=100"
    assert requests[1][0] == "/agent/jobs"
    assert all(headers["X-Device-Serial"] == "DEMO0000001" for _, headers, _ in requests)
    assert requests[1][2] == {
        "kind": kind,
        "orderId": order_id,
        "allocationId": allocation_id,
        "buyerNip": "5260250274",
    }


def document(order_id, order_number, amount="250.00"):
    return {
        "kind": "sale",
        "orderId": order_id,
        "orderNumber": order_number,
        "allocationId": None,
        "amount": amount,
        "customerName": None,
        "occurredAt": JOB["createdAt"],
    }


def test_document_list_keeps_over_limit_and_normal_amounts():
    items = [document(JOB["orderId"], 10, "100000000.00"), document(JOB_ID, 11, "120.00")]
    with server([(200, {"items": items, "nextCursor": None}, {})]) as (url, _requests):
        page = Api(url, "secret", "DEMO0000001").documents()
    assert [item["amount"] for item in page["items"]] == ["100000000.00", "120.00"]


def test_two_page_discovery_passes_the_opaque_cursor_back_unchanged_and_encoded():
    cursor = "a/b+c d&limit=1?#%2F=ż"  # opaque: anything the server chooses must survive
    first = {"items": [document(JOB["orderId"], 10)], "nextCursor": cursor}
    last = {"items": [document(JOB_ID, 11)], "nextCursor": None}
    with server([(200, first, {}), (200, last, {})]) as (url, requests):
        api = Api(url, "secret", "DEMO0000001")
        assert api.documents() == first
        assert api.documents(first["nextCursor"]) == last
    first_url, last_url = (urlsplit(path) for path, _, _ in requests)
    assert first_url.path == last_url.path == "/agent/documents"
    assert parse_qs(first_url.query, strict_parsing=True) == {"limit": ["100"]}
    assert last_url.query == ("limit=100&cursor=a%2Fb%2Bc%20d%26limit%3D1%3F%23%252F%3D%C5%BC")
    assert parse_qs(last_url.query, strict_parsing=True) == {"limit": ["100"], "cursor": [cursor]}


@pytest.mark.parametrize(
    "page",
    [
        {"items": []},  # nextCursor is required, even when null
        {"items": [], "nextCursor": ""},  # an empty cursor would silently restart the list
        {"items": [], "nextCursor": 7},
        {"items": [document(JOB["orderId"], 10)] * 101, "nextCursor": None},  # over `limit`
        {
            "items": [document(JOB["orderId"], 10), document(JOB_ID, 11) | {"amount": "NaN"}],
            "nextCursor": "next",
        },  # one bad item rejects the whole page; nothing partial is returned
    ],
)
def test_malformed_later_page_is_rejected_whole(page):
    with server([(200, page, {})]) as (url, requests), pytest.raises(ValueError):
        Api(url, "secret", "DEMO0000001").documents("previous")
    assert len(requests) == 1


@pytest.mark.parametrize("cursor", ["", 7])
def test_invalid_cursor_is_rejected_before_sending(cursor):
    with server([]) as (url, requests), pytest.raises(ValueError):
        Api(url, "secret", "DEMO0000001").documents(cursor)
    assert requests == []


@pytest.mark.parametrize(
    ("kind", "allocation_id"),
    [("sale", ATTEMPT), ("advance", None), ("advance", "not-uuid"), ("refund", None)],
)
def test_queue_rejects_invalid_document_identity_before_sending(kind, allocation_id):
    with server([]) as (url, requests), pytest.raises(ValueError):
        Api(url, "secret", "DEMO0000001").queue(kind, JOB["orderId"], allocation_id)
    assert requests == []


@pytest.mark.parametrize(
    "changes",
    [
        {"kind": "refund"},
        {"kind": "advance"},
        {"allocationId": ATTEMPT},
        {"amount": "-1.00"},
        {"amount": "NaN"},
        {"amount": "not-a-number"},
        {"occurredAt": "2026-10-16"},
    ],
)
def test_malformed_printable_document_is_rejected(changes):
    item = {
        "kind": "sale",
        "orderId": JOB["orderId"],
        "orderNumber": 10,
        "allocationId": None,
        "amount": "250.00",
        "customerName": None,
        "occurredAt": JOB["createdAt"],
    } | changes
    page = {"items": [item], "nextCursor": None}
    with server([(200, page, {})]) as (url, _requests), pytest.raises(ValueError):
        Api(url, "secret", "DEMO0000001").documents()


def test_nullable_printer_metadata_matches_status_contract():
    payload = copy.deepcopy(STATUS)
    payload["lastSeenAt"] = None
    for field in (
        "deviceSerial",
        "model",
        "firmware",
        "appVersion",
        "maxItemNameLength",
        "maxLinesPerReceipt",
        "allowedCharacters",
    ):
        payload["printer"][field] = None
    with server([(200, payload, {})]) as (url, _requests):
        assert Api(url, "secret", "DEMO0000001").connect()["id"] == PRINTER["id"]
