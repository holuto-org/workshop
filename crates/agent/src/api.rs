//! The Holuto `/agent` API, authenticated by the printer's permanent device token.
//!
//! Every response is decoded into typed values before anything acts on it; a body that does not
//! match the contract is an error, never a partial success. Redirects are refused (they would
//! carry the token elsewhere) and no request is retried here: the agent decides what is safe.

pub mod model;

use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ureq::{
    Agent, Body, RequestBuilder,
    http::{Response, StatusCode},
    tls::{RootCerts, TlsConfig},
};
use uuid::Uuid;

use crate::{
    Error,
    documents::{BuyerNip, Document},
    settings,
};
use model::{Job, Page, PrinterInfo, PrinterStatus, QueueForm, ResultForm, Telemetry};

/// The largest page `/agent/documents` serves.
pub const DOCUMENTS_LIMIT: usize = 100;
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE: u64 = 4 * 1024 * 1024;
const MAX_SERIAL_LENGTH: usize = 32;
/// The backend's `App-Version` gate parses `agent/<SemVer>`.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The token was rejected (rotated or revoked). Processing stops; there is no refresh.
    #[error("Holuto odrzuciło token drukarki (401). Wklej aktualny token w Ustawieniach.")]
    Unauthorized,
    #[error("Holuto odpowiedziało błędem {status}{}", problem(.detail.as_deref()))]
    Status { status: u16, detail: Option<String> },
    #[error("Brak połączenia z Holuto: {0}")]
    Transport(#[from] ureq::Error),
    /// A response the contract does not allow; nothing acts on it.
    #[error("Nieprawidłowa odpowiedź Holuto ({0}).")]
    Response(String),
}

/// What the agent needs from Holuto; implemented over HTTP by [`Api`] and by test doubles.
pub trait Holuto {
    /// The physical serial this connection is bound to (`X-Device-Serial`).
    fn serial(&self) -> &str;
    /// The paired printer, once a session or heartbeat answered.
    fn printer_id(&self) -> Option<Uuid>;
    /// Binds the device serial and replaces the backend's telemetry, VAT slots included.
    fn session(&mut self, telemetry: &Telemetry) -> Result<PrinterInfo, ApiError>;
    fn heartbeat(&mut self, telemetry: &Telemetry) -> Result<PrinterInfo, ApiError>;
    /// Claims the next queued job; `None` when the queue is empty.
    fn take(&mut self) -> Result<Option<Job>, ApiError>;
    /// The job's current state, for reconciliation; never prints.
    fn job(&mut self, id: Uuid) -> Result<Job, ApiError>;
    fn report(&mut self, id: Uuid, result: &ResultForm) -> Result<Job, ApiError>;
    /// One page of eligible documents, oldest first; pass the page's cursor back unchanged.
    fn documents(&mut self, cursor: Option<&str>) -> Result<Page, ApiError>;
    fn queue(&mut self, form: &QueueForm) -> Result<Job, ApiError>;
}

#[derive(Debug)]
pub struct Api {
    agent: Agent,
    base_url: String,
    authorization: String,
    serial: String,
    printer_id: Option<Uuid>,
}

impl Api {
    pub fn new(api_url: &str, token: &str, serial: &str) -> Result<Self, Error> {
        let base_url = settings::api_url(api_url)?;
        let serial_valid = (1..=MAX_SERIAL_LENGTH).contains(&serial.len())
            && serial
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
            && serial.trim() == serial;
        if !serial_valid {
            return Err(Error::Invalid(
                "Numer drukarki musi zawierać 1-32 drukowalne znaki ASCII.".to_owned(),
            ));
        }
        let config = Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .user_agent(format!("holuto-fiscal/{APP_VERSION}"))
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .build();
        Ok(Self {
            agent: Agent::new_with_config(config),
            base_url,
            authorization: format!("Bearer {token}"),
            serial: serial.to_owned(),
            printer_id: None,
        })
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<T>, ApiError> {
        let request = self.headers(self.agent.get(self.url(path)), path);
        decode(request.query_pairs(query.iter().copied()).call()?)
    }

    fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<Option<T>, ApiError> {
        let request = self.headers(self.agent.post(self.url(path)), path);
        decode(match body {
            Some(body) => request.send_json(body),
            None => request.send_empty(),
        }?)
    }

    fn url(&self, path: &str) -> String {
        format!("{}/agent/{path}", self.base_url)
    }

    fn telemetry(&mut self, path: &str, telemetry: &Telemetry) -> Result<PrinterInfo, ApiError> {
        let status: PrinterStatus = self.post(path, Some(telemetry))?.ok_or_else(empty)?;
        let printer = status.printer;
        printer.check(&self.serial).map_err(ApiError::Response)?;
        if self.printer_id.is_some_and(|id| id != printer.id) {
            return Err(ApiError::Response(
                "token przypisany do innej drukarki".to_owned(),
            ));
        }
        self.printer_id = Some(printer.id);
        Ok(printer)
    }

    /// Bearer token, protocol version and language on every request; the device serial on every
    /// request but session and heartbeat, which carry it in the body.
    fn headers<B>(&self, request: RequestBuilder<B>, path: &str) -> RequestBuilder<B> {
        let request = request
            .header("Authorization", &self.authorization)
            .header("App-Version", format!("agent/{APP_VERSION}"))
            .header("Accept-Language", "pl");
        if matches!(path, "session" | "heartbeat") {
            request
        } else {
            request.header("X-Device-Serial", &self.serial)
        }
    }
}

fn decode<T: DeserializeOwned>(response: Response<Body>) -> Result<Option<T>, ApiError> {
    let status = response.status();
    let bytes = response
        .into_body()
        .with_config()
        .limit(MAX_RESPONSE)
        .read_to_vec()?;
    match status {
        StatusCode::NO_CONTENT => Ok(None),
        StatusCode::OK | StatusCode::CREATED => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| ApiError::Response(error.to_string())),
        StatusCode::UNAUTHORIZED => Err(ApiError::Unauthorized),
        other => Err(ApiError::Status {
            status: other.as_u16(),
            detail: problem_detail(&bytes),
        }),
    }
}

/// The localized `detail` of an `application/problem+json` body.
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

impl Holuto for Api {
    fn serial(&self) -> &str {
        &self.serial
    }

    fn printer_id(&self) -> Option<Uuid> {
        self.printer_id
    }

    fn session(&mut self, telemetry: &Telemetry) -> Result<PrinterInfo, ApiError> {
        self.telemetry("session", telemetry)
    }

    fn heartbeat(&mut self, telemetry: &Telemetry) -> Result<PrinterInfo, ApiError> {
        self.telemetry("heartbeat", telemetry)
    }

    fn take(&mut self) -> Result<Option<Job>, ApiError> {
        self.post("jobs/take", None::<&()>)
    }

    fn job(&mut self, id: Uuid) -> Result<Job, ApiError> {
        let job: Job = self.get(&format!("jobs/{id}"), &[])?.ok_or_else(empty)?;
        if job.id != id {
            return Err(ApiError::Response("stan innego zadania".to_owned()));
        }
        Ok(job)
    }

    fn report(&mut self, id: Uuid, result: &ResultForm) -> Result<Job, ApiError> {
        let job: Job = self
            .post(&format!("jobs/{id}/result"), Some(result))?
            .ok_or_else(empty)?;
        let echoed = job.id == id
            && job.attempt == Some(result.attempt)
            && job.state == result.state.into()
            && job.receipt_number == result.receipt_number
            && job.result == result.result;
        if !echoed {
            return Err(ApiError::Response(
                "wynik zadania nie został zapisany zgodnie z raportem".to_owned(),
            ));
        }
        Ok(job)
    }

    fn documents(&mut self, cursor: Option<&str>) -> Result<Page, ApiError> {
        let limit = DOCUMENTS_LIMIT.to_string();
        let mut query = vec![("limit", limit.as_str())];
        if let Some(cursor) = cursor {
            if cursor.is_empty() {
                return Err(ApiError::Response("pusty kursor strony".to_owned()));
            }
            query.push(("cursor", cursor));
        }
        let page: Page = self.get("documents", &query)?.ok_or_else(empty)?;
        page.check().map_err(ApiError::Response)?;
        Ok(page)
    }

    fn queue(&mut self, form: &QueueForm) -> Result<Job, ApiError> {
        self.post("jobs", Some(form))?.ok_or_else(empty)
    }
}

/// Builds the queue request, refusing a document whose identity is inconsistent.
pub fn queue_form(document: &Document, buyer_nip: Option<BuyerNip>) -> Result<QueueForm, Error> {
    document
        .check()
        .map_err(|problem| Error::Invalid(format!("Nieprawidłowy dokument: {problem}.")))?;
    Ok(QueueForm {
        kind: document.kind,
        order_id: document.order_id,
        allocation_id: document.allocation_id,
        buyer_nip,
    })
}

#[cfg(test)]
mod tests;
