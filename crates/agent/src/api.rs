//! Holuto `/agent` pairing and authenticated device API.
//!
//! Rejects malformed responses and bodies over 4 MiB; refuses redirects to protect the token.
//! Retry decisions belong to the agent.
//! TLS diagnostics retain only a category, including certificate errors wrapped in I/O.

#[cfg(any(test, feature = "testing"))]
pub(crate) mod contract;
pub mod model;

use fiscal::device::valid_unique_number;

use std::{
    error::Error as StdError,
    fmt,
    io::{ErrorKind, Read as _},
    time::Duration,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tracing::field::Empty;
use ureq::{
    Agent, Body, RequestBuilder,
    http::{HeaderValue, Response, StatusCode},
    tls::{RootCerts, TlsConfig},
};
use uuid::Uuid;

use crate::{
    Error,
    documents::{BuyerNip, Document},
    settings,
};
use model::{Job, Page, Pairing, PrinterStatus, QueueForm, ResultForm, Telemetry};

/// Backend page-size limit.
pub const DOCUMENTS_LIMIT: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
/// Sent as `agent/<SemVer>` for the backend's `App-Version` gate.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(thiserror::Error)]
pub enum ApiError {
    /// Stops processing; tokens are never refreshed automatically.
    #[error("Holuto odrzuciło parowanie drukarki (401). Sparuj drukarkę ponownie w Ustawieniach.")]
    Unauthorized,
    #[error("Holuto odpowiedziało błędem {status}{}", problem(.detail.as_deref()))]
    Status { status: u16, detail: Option<String> },
    #[error("Błąd komunikacji z Holuto: {0}")]
    Transport(TransportFailure),
    #[error("Nieprawidłowa odpowiedź Holuto ({0}).")]
    Response(String),
}

impl fmt::Debug for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("Unauthorized"),
            Self::Status { status, .. } => f
                .debug_struct("Status")
                .field("status", status)
                .finish_non_exhaustive(),
            Self::Transport(failure) => f.debug_tuple("Transport").field(failure).finish(),
            Self::Response(_) => f.write_str("Response([REDACTED])"),
        }
    }
}

/// Categories only: never retains peer-controlled strings or credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportFailure {
    #[error("Przekroczono czas oczekiwania.")]
    Timeout,
    #[error("Serwer odrzucił połączenie.")]
    ConnectionRefused,
    #[error("Nie udało się nawiązać połączenia.")]
    Connection,
    #[error("Nie można odnaleźć serwera.")]
    Dns,
    #[error("Nie można zweryfikować bezpiecznego połączenia TLS.")]
    Tls,
    #[error("Nieprawidłowa odpowiedź protokołu HTTP.")]
    Protocol,
    #[error("Przerwano transmisję danych.")]
    Io,
    #[error("Nie można połączyć przez skonfigurowany serwer proxy.")]
    Proxy,
}

impl From<ureq::Error> for ApiError {
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "ureq is non-exhaustive; other failures are grouped without retaining peer text"
    )]
    fn from(value: ureq::Error) -> Self {
        let failure = match value {
            ureq::Error::Timeout(_) => TransportFailure::Timeout,
            ureq::Error::HostNotFound => TransportFailure::Dns,
            ureq::Error::ConnectionFailed => TransportFailure::Connection,
            ureq::Error::Io(error) => {
                let kind = error.kind();
                if error
                    .get_ref()
                    .is_some_and(<dyn StdError + Send + Sync>::is::<rustls::Error>)
                {
                    TransportFailure::Tls
                } else if kind == ErrorKind::TimedOut {
                    TransportFailure::Timeout
                } else if kind == ErrorKind::ConnectionRefused {
                    TransportFailure::ConnectionRefused
                } else if matches!(kind, ErrorKind::UnexpectedEof | ErrorKind::InvalidData) {
                    TransportFailure::Protocol
                } else {
                    TransportFailure::Io
                }
            },
            ureq::Error::Tls(_)
            | ureq::Error::Pem(_)
            | ureq::Error::Rustls(_)
            | ureq::Error::TlsRequired
            | ureq::Error::RequireHttpsOnly(_) => TransportFailure::Tls,
            ureq::Error::InvalidProxyUrl | ureq::Error::ConnectProxyFailed(_) => {
                TransportFailure::Proxy
            },
            ureq::Error::BodyExceedsLimit(_) | ureq::Error::LargeResponseHeader(..) => {
                return Self::Response("przekroczony limit rozmiaru odpowiedzi".to_owned());
            },
            _ => TransportFailure::Protocol,
        };
        Self::Transport(failure)
    }
}

pub struct Api {
    agent: Agent,
    base_url: String,
    authorization: settings::Token,
    unique_number: String,
    printer_id: Option<Uuid>,
}

impl fmt::Debug for Api {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Api")
            .field("uniqueNumber", &self.unique_number)
            .field("printer_id", &self.printer_id)
            .finish_non_exhaustive()
    }
}

impl Api {
    /// Exchanges a single-use human code; never sends a token or printer identity.
    pub fn pair(api_url: &str, code: &str) -> Result<Pairing, Error> {
        if api_url.trim().is_empty() {
            return Err(Error::Invalid("Wpisz adres API w Ustawieniach.".to_owned()));
        }
        // Typos must not consume the backend's per-address pairing attempts.
        let code = pairing_code(code).ok_or_else(|| {
            Error::Invalid("Kod parowania ma 8 znaków z Holuto, na przykład ABCD-1234.".to_owned())
        })?;
        let base_url = settings::api_url(api_url)?;
        let span = tracing::info_span!(
            "holuto.request",
            sentry.op = "http.client",
            method = "POST",
            route = "/agent/pair",
            status = Empty
        );
        let _entered = span.enter();
        let request = trace_headers(
            http_agent(&base_url)
                .post(format!("{base_url}/agent/pair"))
                .header("App-Version", format!("agent/{APP_VERSION}"))
                .header("Accept-Language", "pl"),
        )?;
        let response = request
            .send_json(model::PairForm { code })
            .map_err(ApiError::from)?;
        // Malformed responses may contain credentials; discard body and parser text.
        let paired: Pairing = decode(response)
            .map_err(|error| match error {
                ApiError::Response(_) => {
                    ApiError::Response("nieprawidłowe dane parowania".to_owned())
                },
                ApiError::Status { status, .. } => ApiError::Status {
                    status,
                    detail: match status {
                        400 => Some("fiscal_pairing_invalid".to_owned()),
                        429 => Some("rate_limited".to_owned()),
                        _ => None,
                    },
                },
                other @ (ApiError::Unauthorized | ApiError::Transport(_)) => other,
            })?
            .ok_or_else(empty)?;
        let token = paired.token.as_str();
        if token.len() != 43
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !token
                .as_bytes()
                .last()
                .is_some_and(|byte| b"AEIMQUYcgkosw048".contains(byte))
            || paired.printer_id.is_nil()
        {
            return Err(ApiError::Response("nieprawidłowe dane parowania".to_owned()).into());
        }
        Ok(paired)
    }

    pub fn new(api_url: &str, token: &str, unique_number: &str) -> Result<Self, Error> {
        let base_url = settings::api_url(api_url)?;
        if !(1..=4096).contains(&token.len()) || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::Invalid(
                "Sparuj drukarkę kodem z Holuto w Ustawieniach.".to_owned(),
            ));
        }
        let identity_valid = valid_unique_number(unique_number);
        if !identity_valid {
            return Err(Error::Invalid(
                "Numer unikatowy musi zawierać 3 wielkie litery i 10 cyfr.".to_owned(),
            ));
        }
        Ok(Self {
            agent: http_agent(&base_url),
            base_url,
            authorization: format!("Bearer {token}").into(),
            unique_number: unique_number.to_owned(),
            printer_id: None,
        })
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<T>, ApiError> {
        let span = tracing::info_span!(
            "holuto.request",
            sentry.op = "http.client",
            method = "GET",
            route = route(path),
            status = Empty
        );
        let _entered = span.enter();
        let request = self.headers(self.agent.get(self.url(path)), path)?;
        decode(request.query_pairs(query.iter().copied()).call()?)
            .map_err(|error| self.redact(error))
    }

    fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<Option<T>, ApiError> {
        let span = tracing::info_span!(
            "holuto.request",
            sentry.op = "http.client",
            method = "POST",
            route = route(path),
            status = Empty
        );
        let _entered = span.enter();
        let request = self.headers(self.agent.post(self.url(path)), path)?;
        decode(match body {
            Some(body) => request.send_json(body),
            None => request.send_empty(),
        }?)
        .map_err(|error| self.redact(error))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/agent/{path}", self.base_url)
    }

    fn redact(&self, mut error: ApiError) -> ApiError {
        let token = self
            .authorization
            .as_str()
            .strip_prefix("Bearer ")
            .unwrap_or_default();
        match &mut error {
            ApiError::Status {
                detail: Some(detail),
                ..
            }
            | ApiError::Response(detail) => {
                *detail = detail.replace(token, "[REDACTED]");
            },
            ApiError::Unauthorized
            | ApiError::Transport(_)
            | ApiError::Status { detail: None, .. } => {},
        }
        error
    }

    /// Sessions carry the unique number in their body; all other routes use its header.
    fn headers<B>(
        &self,
        request: RequestBuilder<B>,
        path: &str,
    ) -> Result<RequestBuilder<B>, ApiError> {
        let request = request
            .header("Authorization", self.authorization.as_str())
            .header("App-Version", format!("agent/{APP_VERSION}"))
            .header("Accept-Language", "pl");
        let request = if path == "session" {
            request
        } else {
            request.header("X-Device-Unique-Number", &self.unique_number)
        };
        trace_headers(request)
    }

    pub fn unique_number(&self) -> &str {
        &self.unique_number
    }

    pub const fn printer_id(&self) -> Option<Uuid> {
        self.printer_id
    }

    pub fn session(&mut self, telemetry: &Telemetry) -> Result<(), ApiError> {
        let status: PrinterStatus = self.post("session", Some(telemetry))?.ok_or_else(empty)?;
        let printer = status.printer;
        printer
            .check(&self.unique_number)
            .map_err(ApiError::Response)?;
        if self.printer_id.is_some_and(|id| id != printer.id) {
            return Err(ApiError::Response(
                "parowanie przypisane do innej drukarki".to_owned(),
            ));
        }
        self.printer_id = Some(printer.id);
        Ok(())
    }

    pub fn heartbeat(&self) -> Result<(), ApiError> {
        let _: Option<()> = self.post("heartbeat", None::<&()>)?;
        Ok(())
    }

    pub fn take(&self) -> Result<Option<Job>, ApiError> {
        self.post("jobs/take", None::<&()>)
    }

    pub fn job(&self, id: Uuid) -> Result<Job, ApiError> {
        let job: Job = self.get(&format!("jobs/{id}"), &[])?.ok_or_else(empty)?;
        if job.id != id {
            return Err(ApiError::Response("stan innego zadania".to_owned()));
        }
        Ok(job)
    }

    pub fn report(&self, id: Uuid, result: &ResultForm) -> Result<(), ApiError> {
        let job: Job = self
            .post(&format!("jobs/{id}/result"), Some(result))?
            .ok_or_else(empty)?;
        let echoed = job.id == id
            && job.attempt == Some(result.attempt)
            && job.state == result.state.into()
            && job.receipt_number == result.receipt_number;
        if !echoed {
            return Err(ApiError::Response(
                "wynik zadania nie został zapisany zgodnie z raportem".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn documents(&self, cursor: Option<&str>) -> Result<Page, ApiError> {
        let limit = DOCUMENTS_LIMIT.to_string();
        let mut query = vec![("limit", limit.as_str())];
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor));
        }
        let page: Page = self.get("documents", &query)?.ok_or_else(empty)?;
        page.check().map_err(ApiError::Response)?;
        Ok(page)
    }

    /// Parses the unused job to enforce the response contract.
    pub fn queue(&self, form: &QueueForm) -> Result<(), ApiError> {
        let _job: Job = self.post("jobs", Some(form))?.ok_or_else(empty)?;
        Ok(())
    }
}

/// Holuto uses 8 Crockford base32 characters, ignoring ASCII case and hyphens.
fn pairing_code(input: &str) -> Option<String> {
    let code: String = input
        .trim()
        .chars()
        .filter(|character| *character != '-')
        .map(|character| character.to_ascii_uppercase())
        .collect();
    (code.len() == 8
        && code
            .bytes()
            .all(|byte| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte)))
    .then_some(code)
}

fn trace_headers<B>(mut request: RequestBuilder<B>) -> Result<RequestBuilder<B>, ApiError> {
    let headers = request
        .headers_mut()
        .ok_or_else(|| ApiError::Response("nieprawidłowe nagłówki żądania".to_owned()))?;
    // sentry-core emits sentry-trace, not baggage; do not fabricate headers.
    let error = sentry_core::Hub::with_active(|hub| {
        hub.configure_scope(|scope| {
            scope
                .iter_trace_propagation_headers()
                .try_for_each(|(name, value)| {
                    let value = HeaderValue::from_str(&value).map_err(|_invalid| {
                        ApiError::Response("nieprawidłowy kontekst śledzenia".to_owned())
                    })?;
                    let _previous = headers.insert(name, value);
                    Ok::<(), ApiError>(())
                })
                .err()
        })
    });
    error.map_or_else(|| Ok(request), Err)
}

fn http_agent(base_url: &str) -> Agent {
    let loopback = url::Url::parse(base_url)
        .is_ok_and(|url| url.host().as_ref().is_some_and(settings::is_loopback));
    let mut config = Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(TIMEOUT))
        .user_agent(format!("workshop/{APP_VERSION}"))
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        );
    if loopback {
        // Never forward loopback credentials through an environment-configured proxy.
        config = config.proxy(None);
    }
    Agent::new_with_config(config.build())
}

fn route(path: &str) -> &'static str {
    match path {
        "session" => "/agent/session",
        "heartbeat" => "/agent/heartbeat",
        "documents" => "/agent/documents",
        "jobs/take" => "/agent/jobs/take",
        "jobs" => "/agent/jobs",
        other if other.ends_with("/result") => "/agent/jobs/{id}/result",
        _ => "/agent/jobs/{id}",
    }
}

fn decode<T: DeserializeOwned>(response: Response<Body>) -> Result<Option<T>, ApiError> {
    let status = response.status();
    let _recorded = tracing::Span::current().record("status", status.as_u16());
    if status == StatusCode::UNAUTHORIZED {
        return Err(ApiError::Unauthorized);
    }
    let mut bytes = Vec::new();
    let _read = response
        .into_body()
        .into_reader()
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(ureq::Error::from)?;
    if !u64::try_from(bytes.len()).is_ok_and(|length| length <= MAX_RESPONSE) {
        return Err(ApiError::Response(
            "przekroczony limit rozmiaru odpowiedzi".to_owned(),
        ));
    }
    match status {
        StatusCode::NO_CONTENT => Ok(None),
        StatusCode::OK | StatusCode::CREATED => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| ApiError::Response(error.to_string())),
        other => Err(ApiError::Status {
            status: other.as_u16(),
            detail: problem_detail(&bytes),
        }),
    }
}

/// Localized `application/problem+json` detail.
fn problem_detail(bytes: &[u8]) -> Option<String> {
    #[derive(Deserialize)]
    struct Problem {
        detail: String,
    }
    serde_json::from_slice::<Problem>(bytes)
        .ok()
        .map(|problem| problem.detail)
        .filter(|detail| !detail.is_empty())
}

fn problem(detail: Option<&str>) -> String {
    detail
        .map(|detail| format!(": {detail}"))
        .unwrap_or_default()
}

fn empty() -> ApiError {
    ApiError::Response("pusta odpowiedź".to_owned())
}

/// Rejects inconsistent document identities before queueing.
pub fn queue_form(document: &Document, buyer_nip: Option<BuyerNip>) -> Result<QueueForm, Error> {
    document
        .check()
        .map_err(|problem| Error::Invalid(format!("Nieprawidłowy dokument: {problem}.")))?;
    Ok(QueueForm {
        allocation_id: document.allocation_id,
        buyer_nip: buyer_nip.map(BuyerNip::into_string),
        kind: document.kind,
        order_id: document.order_id,
    })
}

#[cfg(test)]
mod tests;
