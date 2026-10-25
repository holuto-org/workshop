//! Stateful loopback Holuto service, used through the production HTTP client.
//!
//! Mirrors the `/agent` contract: routes, authentication, serial headers and
//! binding, inflight exclusion, attempt/result idempotency, unknown upgrades,
//! staff-only resolution, a fresh attempt on take and future issue times.
//! Fixtures intentionally contain synthetic customer data only.

#[cfg(test)]
pub(crate) mod printers;

use crate::api::contract;
#[cfg(test)]
use crate::api::model::Telemetry;
#[cfg(test)]
use crate::store::StateDir;
#[cfg(test)]
use fiscal::device::{Device, Vendor};
use icu_normalizer::ComposingNormalizerBorrowed;
use jiff::Timestamp;
#[cfg(test)]
use posnet::capabilities::capabilities;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    io::{self, Write},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        mpsc::{Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[cfg(test)]
use tempfile::TempDir;
use tiny_http::{Header, Request, Response, Server, StatusCode};
use uuid::Uuid;

pub const JOB_ID: Uuid = Uuid::from_u128(7);
pub const ATTEMPT: Uuid = Uuid::from_u128(8);
pub const PRINTER_ID: Uuid = Uuid::from_u128(9);
pub const ORDER_ID: Uuid = Uuid::from_u128(10);
pub const ALLOCATION_ID: Uuid = Uuid::from_u128(12);
const TOKEN: &str = "synthetic-device-token";

/// Posnet telemetry for the pairing `serial`, as the agent sends it once connected.
#[cfg(test)]
pub(crate) fn telemetry(serial: &str) -> Telemetry {
    let mut telemetry = Telemetry::new(serial, Vendor::Posnet);
    let _skipped = telemetry.describe(&Device {
        vendor: Vendor::Posnet,
        model: "POSNET TEMO ONLINE".to_owned(),
        firmware: "32.01".to_owned(),
        serial: serial.to_owned(),
        problems: Vec::new(),
        vat: Vec::new(),
        capabilities: capabilities(),
    });
    telemetry
}

#[derive(Debug)]
pub enum Fault {
    Problem {
        status: u16,
        code: String,
        detail: String,
    },
    Body {
        status: u16,
        body: Vec<u8>,
    },
    DropAfterCommit,
    Redirect(String),
    Oversized,
    /// Deterministic request delay: signals arrival, waits for explicit release.
    Hold {
        reached: Sender<()>,
        release: Receiver<()>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestLog {
    pub method: String,
    pub path: String,
    pub body: Value,
    /// Authorization is checked but never retained in diagnostics.
    pub authenticated: bool,
    pub serial: Option<String>,
    pub app_version: Option<String>,
    pub language: Option<String>,
    pub sentry_trace: Option<String>,
    pub baggage: Option<String>,
    pub trace_headers: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct State {
    serial: Option<String>,
    unauthorized: bool,
    vendor: Option<String>,
    pairing: Option<(String, Instant)>,
    pairing_throttled: bool,
    pairing_attempts: VecDeque<Instant>,
    paired_token: Option<String>,
    token_generation: u64,
    jobs: HashMap<Uuid, Value>,
    queue_job: Option<Value>,
    queue: VecDeque<Uuid>,
    reported: HashMap<Uuid, Value>,
    documents: Vec<Value>,
    pages: HashMap<Option<String>, Value>,
    faults: VecDeque<(String, String, Fault)>,
    requests: Vec<RequestLog>,
    telemetry: Value,
    attempt: u128,
}

#[expect(
    clippy::expect_used,
    reason = "a poisoned HTTP fake is a failed test, never recoverable state"
)]
fn locked(state: &(Mutex<State>, Condvar)) -> MutexGuard<'_, State> {
    state.0.lock().expect("loopback Holuto state poisoned")
}

pub struct Holuto {
    url: String,
    server: Arc<Server>,
    state: Arc<(Mutex<State>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for Holuto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Holuto").finish_non_exhaustive()
    }
}

impl Default for Holuto {
    fn default() -> Self {
        Self::new()
    }
}

impl Holuto {
    #[expect(
        clippy::panic,
        reason = "HTTP fake startup failure must fail the test immediately"
    )]
    pub fn new() -> Self {
        Self::bind().unwrap_or_else(|error| panic!("cannot start loopback Holuto: {error}"))
    }

    fn bind() -> io::Result<Self> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(io::Error::other)?);
        let url = format!("http://{}", server.server_addr());
        let state = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let peer = Arc::clone(&server);
        let shared = Arc::clone(&state);
        let thread = thread::Builder::new()
            .name("loopback Holuto".to_owned())
            .spawn(move || {
                while let Ok(request) = peer.recv() {
                    handle(request, &shared);
                }
            })?;
        Ok(Self {
            url,
            server,
            state,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    #[expect(
        clippy::unused_self,
        reason = "fake credentials use the same instance-bound API as its URL"
    )]
    pub const fn token(&self) -> &'static str {
        TOKEN
    }

    #[expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "enqueue accepts a synthetic job fixture with its required identity fields"
    )]
    pub fn enqueue(&self, mut job: Value) -> Uuid {
        let id: Uuid = serde_json::from_value(job["id"].clone()).unwrap();
        job["state"] = json!("queued");
        job["attempt"] = Value::Null;
        job["takenAt"] = Value::Null;
        let mut state = locked(&self.state);
        let _old = state.jobs.insert(id, job);
        state.queue.push_back(id);
        id
    }

    /// Installs durable server evidence for crash/restart scenarios, without queueing it.
    #[expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "recovery fixtures require a valid job identity"
    )]
    pub fn insert(&self, job: Value) {
        let id = serde_json::from_value(job["id"].clone()).unwrap();
        let _old = locked(&self.state).jobs.insert(id, job);
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "test requests evidence for a job it inserted"
    )]
    pub fn job(&self, id: Uuid) -> Value {
        locked(&self.state).jobs[&id].clone()
    }

    #[expect(
        clippy::unwrap_used,
        reason = "test fixture patches must target an inserted JSON object"
    )]
    pub fn set_job(&self, id: Uuid, changes: &Value) {
        let mut state = locked(&self.state);
        let job = state.jobs.get_mut(&id).unwrap();
        job.as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
    }

    pub fn documents(&self, documents: Vec<Value>) {
        locked(&self.state).documents = documents;
    }

    /// Synthetic backend snapshot used by queue requests.
    pub fn queue_job(&self, job: Value) {
        locked(&self.state).queue_job = Some(job);
    }

    pub fn page(&self, cursor: Option<&str>, page: Value) {
        let _old = locked(&self.state)
            .pages
            .insert(cursor.map(str::to_owned), page);
    }

    pub fn unauthorized(&self, unauthorized: bool) {
        locked(&self.state).unauthorized = unauthorized;
    }

    /// Issues a new 15-minute proof for the synthetic printer, replacing the previous proof.
    #[expect(
        clippy::expect_used,
        reason = "test fixtures must use a valid Crockford code"
    )]
    pub fn pairing_code(&self, code: &str) {
        let code = pairing_code(code).expect("invalid synthetic pairing code");
        locked(&self.state).pairing = Some((code, Instant::now() + Duration::from_mins(15)));
    }

    pub fn expire_pairing_code(&self, code: &str) {
        let mut state = locked(&self.state);
        if let Some((current, expires)) = &mut state.pairing
            && Some(current.as_str()) == pairing_code(code).as_deref()
        {
            *expires = Instant::now();
        }
    }

    pub fn pairing_throttled(&self, throttled: bool) {
        locked(&self.state).pairing_throttled = throttled;
    }

    pub fn fault(&self, method: &str, path: &str, fault: Fault) {
        locked(&self.state)
            .faults
            .push_back((method.to_owned(), path.to_owned(), fault));
    }

    pub fn clear_faults(&self) {
        locked(&self.state).faults.clear();
    }

    pub fn requests(&self) -> Vec<RequestLog> {
        locked(&self.state).requests.clone()
    }

    #[expect(
        clippy::unwrap_used,
        reason = "a poisoned request barrier is a failed test"
    )]
    pub fn wait_for_requests(&self, count: usize) -> Vec<RequestLog> {
        let wake = &self.state.1;
        let state = locked(&self.state);
        let (state, timeout) = wake
            .wait_timeout_while(state, Duration::from_secs(10), |state| {
                state.requests.len() < count
            })
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "expected {count} Holuto requests, got {}",
            state.requests.len()
        );
        state.requests.clone()
    }

    /// Staff can resolve only UNKNOWN; attempts stay unchanged.
    pub fn resolve(&self, id: Uuid, receipt_number: Option<&str>) -> Result<(), &'static str> {
        let mut state = locked(&self.state);
        let job = state.jobs.get_mut(&id).ok_or("fiscal_not_found")?;
        if job["state"] != "unknown" {
            return Err("fiscal_job_conflict");
        }
        job["state"] = json!(if receipt_number.is_some() {
            "done"
        } else {
            "failed"
        });
        job["receiptNumber"] = json!(receipt_number);
        job["result"] = json!("Sprawdzono wydruk");
        job["resolvedAt"] = json!(Timestamp::now());
        Ok(())
    }

    /// Only queued jobs become EXPIRED.
    /// A timed-out TAKEN attempt becomes UNKNOWN and can report DONE or UNKNOWN.
    #[expect(clippy::unwrap_used, reason = "expiry targets an inserted fixture")]
    pub fn expire(&self, id: Uuid) {
        let mut state = locked(&self.state);
        let job = state.jobs.get_mut(&id).unwrap();
        match job["state"].as_str() {
            Some("queued") => job["state"] = json!("expired"),
            Some("taken") => job["state"] = json!("unknown"),
            _ => {},
        }
    }
}

impl Drop for Holuto {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _result = thread.join();
        }
    }
}

fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str().to_owned())
}

#[expect(
    clippy::unwrap_used,
    reason = "fake schema/header failures must fail the boundary test"
)]
fn handle(mut request: Request, shared: &Arc<(Mutex<State>, Condvar)>) {
    let method = request.method().as_str().to_owned();
    let path = request.url().to_owned();
    let mut bytes = Vec::new();
    if request.as_reader().read_to_end(&mut bytes).is_err() {
        return;
    }
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let token = locked(shared).paired_token.clone();
    let log = RequestLog {
        method: method.clone(),
        path: path.clone(),
        body,
        authenticated: header(&request, "Authorization").as_deref()
            == Some(&format!("Bearer {}", token.as_deref().unwrap_or(TOKEN))),
        serial: header(&request, "X-Device-Serial"),
        app_version: header(&request, "App-Version"),
        language: header(&request, "Accept-Language"),
        sentry_trace: header(&request, "sentry-trace"),
        baggage: header(&request, "baggage"),
        trace_headers: request
            .headers()
            .iter()
            .filter_map(|header| {
                let name = header.field.as_str().as_str();
                (name.eq_ignore_ascii_case("baggage")
                    || name
                        .get(..7)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sentry-")))
                .then(|| (name.to_ascii_lowercase(), header.value.as_str().to_owned()))
            })
            .collect(),
    };
    let mut state = locked(shared);
    state.requests.push(log.clone());
    shared.1.notify_all();
    let route = path.split('?').next().unwrap_or(&path);
    let position = state
        .faults
        .iter()
        .position(|(m, p, _)| m == &method && (p == &path || p == route));
    let fault = position
        .and_then(|index| state.faults.remove(index))
        .map(|(_, _, fault)| fault);
    drop(state);
    let fault = match fault {
        Some(Fault::Hold { reached, release }) => {
            let _sent = reached.send(());
            let _released = release.recv();
            None
        },
        other => other,
    };
    if let Some(Fault::Problem {
        status,
        code,
        detail,
    }) = &fault
    {
        let body = problem(*status, code, detail);
        contract::validate_response(&method, &path, *status, &body).unwrap();
        respond(request, *status, &body);
        return;
    }
    if let Some(Fault::Body { status, body }) = fault {
        respond_bytes(request, status, body);
        return;
    }
    if let Some(Fault::Redirect(location)) = &fault {
        let response = Response::empty(302)
            .with_header(Header::from_bytes("Location", location.as_str()).unwrap());
        let _sent = request.respond(response);
        return;
    }
    if matches!(fault, Some(Fault::Oversized)) {
        respond_bytes(request, 200, vec![b'x'; 4 * 1024 * 1024 + 1]);
        return;
    }
    contract::validate_request(&method, &path, &log.body).unwrap();
    let (status, body) = route_request(&mut locked(shared), &log);
    contract::validate_response(&method, &path, status, &body).unwrap();
    if matches!(fault, Some(Fault::DropAfterCommit)) {
        // Terminate the HTTP acknowledgement at framing level, so ureq observes a
        // transport failure immediately rather than waiting for a truncated body.
        let mut writer = request.into_writer();
        let _sent = writer.write_all(b"lost acknowledgement\r\n\r\n");
        let _flushed = writer.flush();
        return;
    }
    respond(request, status, &body);
}

fn problem(status: u16, code: &str, detail: &str) -> Value {
    json!({"type":"about:blank","title":"Holuto","status":status,"code":code,"detail":detail})
}
#[expect(
    clippy::unwrap_used,
    reason = "serializing a synthetic JSON Value cannot fail"
)]
fn respond(request: Request, status: u16, body: &Value) {
    respond_bytes(request, status, serde_json::to_vec(body).unwrap());
}
#[expect(
    clippy::unwrap_used,
    reason = "the static Content-Type header is valid ASCII"
)]
fn respond_bytes(request: Request, status: u16, body: Vec<u8>) {
    let response = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
    let _sent = request.respond(response);
}
fn refused(status: u16, code: &str) -> (u16, Value) {
    (status, problem(status, code, code))
}

fn invalid(pointer: &str, code: &str) -> (u16, Value) {
    (
        400,
        json!({
            "type": "about:blank", "title": "Holuto", "status": 400,
            "code": "validation_failed", "detail": "Nieprawidłowe dane.",
            "errors": [{"pointer": pointer, "code": code, "detail": code}]
        }),
    )
}

fn pairing_code(input: &str) -> Option<String> {
    let normalized: String = input
        .bytes()
        .filter(|byte| *byte != b'-')
        .map(|byte| char::from(byte.to_ascii_uppercase()))
        .collect();
    (normalized.len() == 8
        && normalized
            .bytes()
            .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte)))
    .then_some(normalized)
}

fn pair(state: &mut State, request: &RequestLog) -> (u16, Value) {
    let now = Instant::now();
    while state
        .pairing_attempts
        .front()
        .is_some_and(|at| now.duration_since(*at) >= Duration::from_mins(15))
    {
        let _expired = state.pairing_attempts.pop_front();
    }
    if state.pairing_throttled || state.pairing_attempts.len() >= 10 {
        return refused(429, "rate_limited");
    }
    state.pairing_attempts.push_back(now);
    let code = request
        .body
        .get("code")
        .and_then(Value::as_str)
        .and_then(pairing_code);
    let valid = state.pairing.as_ref().is_some_and(|(expected, expires)| {
        code.as_deref() == Some(expected.as_str()) && now < *expires
    });
    if !valid {
        return refused(400, "fiscal_pairing_invalid");
    }
    state.pairing = None;
    state.token_generation += 1;
    let token = format!("{:0>42}A", state.token_generation);
    state.paired_token = Some(token.clone());
    (200, json!({"token":token, "printerId":PRINTER_ID}))
}

// Conflicts: an UNKNOWN job on the same printer, an active job for the same
// document, or an open job of the other kind for the same order.
fn blocks_queue(existing: &Value, requested: &Value) -> bool {
    let state = existing["state"].as_str();
    let active = matches!(state, Some("queued" | "taken" | "done" | "unknown"));
    (existing["printerId"] == requested["printerId"] && state == Some("unknown"))
        || (existing["orderId"] == requested["orderId"]
            && ((existing["kind"] == requested["kind"]
                && existing["allocationId"] == requested["allocationId"]
                && active)
                || (requested["kind"] == "sale"
                    && existing["kind"] == "advance"
                    && matches!(state, Some("queued" | "taken" | "unknown")))
                || (requested["kind"] == "advance" && existing["kind"] == "sale" && active)))
}

// Active (queued/taken/done/unknown) jobs for the same order hide a document:
// sales by any sale or unfinished advance, advances by a sale or the same allocation.
fn document_available(document: &Value, jobs: &HashMap<Uuid, Value>) -> bool {
    !jobs.values().any(|job| {
        job["orderId"] == document["orderId"]
            && matches!(
                job["state"].as_str(),
                Some("queued" | "taken" | "done" | "unknown")
            )
            && if document["kind"] == "sale" {
                job["kind"] == "sale" || (job["kind"] == "advance" && job["state"] != "done")
            } else {
                job["kind"] == "sale" || job["allocationId"] == document["allocationId"]
            }
    })
}

#[expect(
    clippy::unwrap_used,
    reason = "document fixtures must carry valid timestamps and UUID identities"
)]
fn document_key(document: &Value) -> (Timestamp, Uuid, bool, Uuid) {
    let order: Uuid = document["orderId"].as_str().unwrap().parse().unwrap();
    let allocation = document["allocationId"]
        .as_str()
        .map(|id| id.parse::<Uuid>().unwrap());
    (
        document["occurredAt"].as_str().unwrap().parse().unwrap(),
        order,
        document["kind"] == "sale",
        allocation.unwrap_or(order),
    )
}

fn documents_page(state: &State, url: &url::Url) -> (u16, Value) {
    let cursor = url
        .query_pairs()
        .find(|(name, _)| name == "cursor")
        .map(|(_, value)| value.into_owned());
    if let Some(page) = state.pages.get(&cursor) {
        return (200, page.clone());
    }
    let after = match cursor {
        None => None,
        Some(cursor) => {
            let Ok(id) = Uuid::parse_str(&cursor) else {
                return refused(400, "invalid_cursor");
            };
            // Resolve the anchor before eligibility removes queued documents.
            let Some(key) = state
                .documents
                .iter()
                .map(document_key)
                .find(|key| key.3 == id)
            else {
                return (200, json!({"items": [], "nextCursor": null}));
            };
            Some(key)
        },
    };
    let limit = url
        .query_pairs()
        .find(|(name, _)| name == "limit")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    // Ordered by the document key; advance sorts before sale.
    let mut eligible: Vec<_> = state
        .documents
        .iter()
        .filter(|document| document_available(document, &state.jobs))
        .map(|document| (document_key(document), document))
        .filter(|(key, _)| after.is_none_or(|after| *key > after))
        .collect();
    eligible.sort_unstable_by_key(|(key, _)| *key);
    let items: Vec<_> = eligible
        .iter()
        .take(limit)
        .map(|(_, document)| *document)
        .collect();
    // A next cursor exists only when another eligible document follows.
    let next = eligible
        .get(limit)
        .and_then(|_| eligible.get(limit - 1))
        .map(|(key, _)| key.3.to_string());
    (200, json!({"items": items, "nextCursor": next}))
}

#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "request and response shapes are validated against vendored OpenAPI at the boundary"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one audited router keeps the fake's coupled job transitions together"
)]
fn route_request(state: &mut State, request: &RequestLog) -> (u16, Value) {
    if request.path == "/agent/pair" && request.method == "POST" {
        return pair(state, request);
    }
    if state.unauthorized || !request.authenticated {
        return refused(401, "unauthorized");
    }
    let url = url::Url::parse(&format!("http://localhost{}", request.path)).unwrap();
    let path = url.path();
    if matches!(path, "/agent/session" | "/agent/heartbeat") && request.method == "POST" {
        let Some(serial) = request.body["serial"].as_str() else {
            return refused(400, "validation_failed");
        };
        if state.serial.as_deref().is_some_and(|bound| bound != serial) {
            return refused(403, "fiscal_serial_mismatch");
        }
        let vendor = request.body["vendor"].as_str().unwrap();
        if state.vendor.as_deref().is_some_and(|bound| bound != vendor) {
            return refused(403, "fiscal_vendor_mismatch");
        }
        state.vendor = Some(vendor.to_owned());
        state.serial = Some(serial.to_owned());
        state.telemetry = request.body.clone();
        let mut status = status_json();
        status["printer"]["deviceSerial"] = json!(serial);
        for name in [
            "vendor",
            "maxQuantityDecimals",
            "capabilities",
            "model",
            "firmware",
            "appVersion",
            "vatSlots",
            "maxItemNameLength",
            "maxLinesPerReceipt",
            "maxCompanyNameLength",
            "allowedCharacters",
        ] {
            status["printer"][name] = request.body[name].clone();
        }
        return (200, status);
    }
    if state.serial.as_deref() != request.serial.as_deref() || state.serial.is_none() {
        return refused(403, "fiscal_serial_mismatch");
    }
    if path == "/agent/documents" && request.method == "GET" {
        return documents_page(state, &url);
    }
    if path == "/agent/jobs/take" && request.method == "POST" {
        if state
            .jobs
            .values()
            .any(|job| matches!(job["state"].as_str(), Some("taken" | "unknown")))
        {
            return (204, Value::Null);
        }
        while let Some(id) = state.queue.pop_front() {
            let job = state.jobs.get_mut(&id).unwrap();
            if job["state"] != "queued" {
                continue;
            }
            state.attempt += 1;
            job["attempt"] = json!(Uuid::from_u128(8 + state.attempt));
            job["takenAt"] = json!(Timestamp::now());
            job["state"] = json!("taken");
            return (200, job.clone());
        }
        return (204, Value::Null);
    }
    if path == "/agent/jobs" && request.method == "POST" {
        if [
            "vendor",
            "maxQuantityDecimals",
            "capabilities",
            "maxItemNameLength",
            "maxLinesPerReceipt",
            "maxCompanyNameLength",
            "allowedCharacters",
        ]
        .iter()
        .any(|field| state.telemetry[field].is_null())
        {
            return refused(409, "fiscal_printer_limits_unknown");
        }
        let mut job = state.queue_job.clone().unwrap_or_else(job_json);
        let id = Uuid::from_u128(100 + u128::try_from(state.jobs.len()).unwrap());
        job["id"] = json!(id);
        for field in ["orderId", "kind", "allocationId", "buyerNip"] {
            job[field] = request.body[field].clone();
        }
        if state
            .jobs
            .values()
            .any(|existing| blocks_queue(existing, &job))
        {
            return refused(409, "fiscal_job_conflict");
        }
        // Advances require a printer that advertises advance deductions.
        if !job["advances"].as_array().unwrap().is_empty()
            && state.telemetry["capabilities"]["advanceDeductions"] == false
        {
            return refused(409, "fiscal_printer_advances_unsupported");
        }
        if job["payments"].as_array().unwrap().is_empty()
            && state.telemetry["capabilities"]["zeroTotalReceipts"] == false
        {
            return refused(409, "fiscal_printer_zero_total_unsupported");
        }
        let decimals = state.telemetry["maxQuantityDecimals"].as_u64().unwrap();
        if job["lines"].as_array().unwrap().iter().any(|line| {
            line["quantity"]
                .as_str()
                .unwrap()
                .split_once('.')
                .is_some_and(|(_, fraction)| {
                    u64::try_from(fraction.trim_end_matches('0').len()).unwrap() > decimals
                })
        }) {
            return refused(409, "fiscal_printer_quantity_precision");
        }
        job["state"] = json!("queued");
        job["attempt"] = Value::Null;
        job["takenAt"] = Value::Null;
        let _old = state.jobs.insert(id, job.clone());
        state.queue.push_back(id);
        return (201, job);
    }
    let Some(tail) = path.strip_prefix("/agent/jobs/") else {
        return refused(404, "fiscal_not_found");
    };
    let mut parts = tail.split('/');
    let Some(id) = parts.next().and_then(|value| Uuid::parse_str(value).ok()) else {
        return refused(404, "fiscal_not_found");
    };
    let Some(job) = state.jobs.get_mut(&id) else {
        return refused(404, "fiscal_not_found");
    };
    match (request.method.as_str(), parts.next()) {
        ("GET", None) => (200, job.clone()),
        ("POST", Some("result")) => {
            let mut form = request.body.clone();
            let done = form["state"] == "done";
            if form["receiptNumber"].is_null() == done || form["issuedAt"].is_null() == done {
                return refused(400, "validation_failed");
            }
            if let Some(at) = form["issuedAt"].as_str()
                && !at
                    .parse::<Timestamp>()
                    .is_ok_and(|at| at <= Timestamp::now())
            {
                return invalid("/issuedAt", "event_time_in_future");
            }
            if let Some(note) = form["result"].as_str() {
                let multiline = note
                    .replace("\r\n", "\n")
                    .replace(['\u{2028}', '\u{2029}'], "\n");
                form["result"] = json!(
                    ComposingNormalizerBorrowed::new_nfc()
                        .normalize(&multiline)
                        .trim()
                );
            }
            if job["attempt"] != form["attempt"] {
                return refused(409, "fiscal_job_conflict");
            }
            if let Some(prior) = state.reported.get(&id) {
                if prior == &form {
                    return (200, job.clone());
                }
                if job["state"] != "unknown" || !done {
                    return refused(409, "fiscal_job_conflict");
                }
            }
            if job["state"] != "taken" && !(job["state"] == "unknown" && form["state"] != "failed")
            {
                return refused(409, "fiscal_job_conflict");
            }
            for name in ["state", "receiptNumber", "result"] {
                job[name] = form[name].clone();
            }
            let _old = state.reported.insert(id, form);
            (200, job.clone())
        },
        _ => refused(404, "fiscal_not_found"),
    }
}

pub fn job_json() -> Value {
    json!({
        "id": JOB_ID, "orderId": ORDER_ID, "orderNumber": 10, "printerId": PRINTER_ID,
        "state": "taken", "kind": "sale", "allocationId": null, "advances": [],
        "payments": [{"form": "card", "amount": "250.00"}],
        "companyName": "Warsztat Kowalski", "buyerNip": null, "total": "250.00",
        "lines": [
            {"name": "Wymiana oleju", "quantity": "2", "unitPrice": "100.00",
             "total": "200.00", "vatRate": "23", "vatSlot": 0},
            {"name": "Klocki hamulcowe", "quantity": "1", "unitPrice": "50.00",
             "total": "50.00", "vatRate": "8", "vatSlot": 1}
        ],
        "attempt": ATTEMPT, "receiptNumber": null, "result": null,
        "createdAt": "2026-10-16T10:00:00Z", "takenAt": "2026-10-16T10:01:00Z",
        "resolvedAt": null, "etag": "0123456789abcdef0123456789abcdef"
    })
}
#[expect(clippy::unwrap_used, reason = "fixture overlays require JSON objects")]
pub fn with(mut value: Value, changes: &Value) -> Value {
    value
        .as_object_mut()
        .unwrap()
        .extend(changes.as_object().unwrap().clone());
    value
}
pub fn printer_json() -> Value {
    json!({
        "id": PRINTER_ID, "branchId": Uuid::from_u128(11), "name": "Workshop",
        "vendor": "posnet", "maxQuantityDecimals": 8,
        "capabilities": {"advanceDeductions": true, "zeroTotalReceipts": true},
        "deviceSerial": "DEMO0000001", "model": "POSNET TEMO ONLINE", "firmware": "32.01",
        "appVersion": "0.2.0", "vatSlots": [], "maxItemNameLength": 80,
        "maxLinesPerReceipt": 500, "maxCompanyNameLength": 40,
        "allowedCharacters": "ABCabc 0123456789.,-ĄąĘęŁłŃńÓóŚśŹźŻżĆć"
    })
}
pub fn status_json() -> Value {
    json!({
        "printer": printer_json(), "isOnline": true,
        "lastSeenAt": "2026-10-16T10:00:00Z", "updatedAt": "2026-10-16T09:00:00Z"
    })
}
#[cfg(test)]
pub(crate) fn state_dir() -> (TempDir, StateDir) {
    let temporary = TempDir::new().unwrap();
    let dir = StateDir::open(temporary.path().join("state")).unwrap();
    (temporary, dir)
}

#[cfg(test)]
mod tests;
