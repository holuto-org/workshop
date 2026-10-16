"""Real HTTP exchanges for the permanent-token, UUID-attempt protocol."""

import copy
import json
import threading
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError

import pytest
from posnet.printer import Printer
from posnet.simulator import Simulator
from test_agent import ATTEMPT, JOB, JOB_ID, PRINTER
from workshop_agent.agent import Agent, Api


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
    return (200, copy.deepcopy(PRINTER), {})


def test_session_heartbeat_take_status_result_use_permanent_bearer_and_clean_paths():
    body = {
        "attempt": ATTEMPT,
        "state": "done",
        "receiptNumber": None,
        "issuedAt": "2026-10-16T10:02:00Z",
        "result": None,
    }
    done = {**JOB, "state": "done", "resolvedAt": body["issuedAt"]}
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
    assert all(
        headers["App-Version"] == f"agent/{api.telemetry['agentVersion']}"
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
    assert requests[0][1].get("X-Device-Serial") is None
    assert all(headers["X-Device-Serial"] == "DEMO0000001" for _, headers, _ in requests[1:])
    assert requests[0][2] == requests[1][2] == api.telemetry
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
        {**PRINTER, "deviceSerial": "OTHER"},
        {**PRINTER, "isOnline": 1},
        {**PRINTER, "id": 7},
        {**PRINTER, "vatRates": [{"index": True, "percent": "23"}]},
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
            "payload": {
                "companyName": "x",
                "lines": [{"name": "x", "quantity": 1, "unitPrice": "10.00"}],
            },
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
            Agent(printer, api).run_forever()
        assert exc.value.code == 401
        assert len(requests) == 1
        assert requests[0][0] == "/agent/heartbeat"
        assert sim.receipts == []
