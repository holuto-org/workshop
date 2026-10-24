//! Fixtures shared by the crate's tests: a Holuto double, a sample job and a state directory.

use std::collections::{HashMap, VecDeque};

use serde_json::{Value, json};
use tempfile::TempDir;
use uuid::Uuid;

use crate::{
    api::{
        ApiError, Holuto,
        model::{Job, Page, QueueForm, ResultForm, Telemetry},
    },
    store::StateDir,
};

pub(crate) const JOB_ID: Uuid = Uuid::from_u128(7);
pub(crate) const ATTEMPT: Uuid = Uuid::from_u128(8);
pub(crate) const PRINTER_ID: Uuid = Uuid::from_u128(9);
pub(crate) const ORDER_ID: Uuid = Uuid::from_u128(10);
pub(crate) const ALLOCATION_ID: Uuid = Uuid::from_u128(12);

pub(crate) fn state_dir() -> (TempDir, StateDir) {
    let temporary = TempDir::new().unwrap();
    let dir = StateDir::open(temporary.path().join("state")).unwrap();
    (temporary, dir)
}

/// A taken sale job for the simulator's default VAT table: 2 × 100 zł at 23 % (slot A) and
/// 50 zł at 8 % (slot B), paid by card.
pub(crate) fn job_json() -> Value {
    json!({
        "id": JOB_ID,
        "orderId": ORDER_ID,
        "orderNumber": 10,
        "printerId": PRINTER_ID,
        "state": "taken",
        "kind": "sale",
        "allocationId": null,
        "advances": [],
        "payments": [{"form": "card", "amount": "250.00"}],
        "companyName": "Warsztat Kowalski",
        "buyerNip": null,
        "total": "250.00",
        "lines": [
            {
                "name": "Wymiana oleju", "quantity": "2", "unitPrice": "100.00", "total": "200.00",
                "vatRate": "23", "vatSlot": 0
            },
            {
                "name": "Klocki hamulcowe", "quantity": "1", "unitPrice": "50.00", "total": "50.00",
                "vatRate": "8", "vatSlot": 1
            }
        ],
        "attempt": ATTEMPT,
        "receiptNumber": null,
        "result": null,
        "createdAt": "2026-10-16T10:00:00Z",
        "takenAt": "2026-10-16T10:01:00Z",
        "resolvedAt": null,
        "etag": "0123456789abcdef0123456789abcdef"
    })
}

/// `value` with the top-level fields of `changes` replaced or added.
pub(crate) fn with(mut value: Value, changes: &Value) -> Value {
    value
        .as_object_mut()
        .unwrap()
        .extend(changes.as_object().unwrap().clone());
    value
}

/// `job_json` with top-level fields replaced.
pub(crate) fn job_with(changes: &Value) -> Job {
    serde_json::from_value(with(job_json(), changes)).unwrap()
}

pub(crate) fn job() -> Job {
    job_with(&json!({}))
}

/// An agent's listener for what the printer waits for, where no test looks.
pub(crate) fn unheard(_problem: Option<&str>) {}

pub(crate) fn printer_json() -> Value {
    json!({
        "id": PRINTER_ID,
        "branchId": Uuid::from_u128(11),
        "name": "Workshop",
        "deviceSerial": "DEMO0000001",
        "model": "POSNET TEMO ONLINE",
        "firmware": "32.01",
        "appVersion": "0.2.0",
        "vatSlots": [],
        "maxItemNameLength": 80,
        "maxLinesPerReceipt": 500,
        "maxCompanyNameLength": 40,
        "allowedCharacters": "ABCabc 0123456789.,-ĄąĘęŁłŃńÓóŚśŹźŻżĆć"
    })
}

pub(crate) fn status_json() -> Value {
    json!({
        "printer": printer_json(),
        "isOnline": true,
        "lastSeenAt": "2026-10-16T10:00:00Z",
        "updatedAt": "2026-10-16T09:00:00Z"
    })
}

/// Holuto as the backend behaves: jobs to hand out, each job's server state, and every
/// heartbeat and result received. A reported result becomes the job's server state.
#[derive(Debug)]
pub(crate) struct FakeApi {
    pub(crate) jobs: VecDeque<Job>,
    pub(crate) remote: HashMap<Uuid, Job>,
    pub(crate) reports: Vec<ResultForm>,
    pub(crate) heartbeats: Vec<Telemetry>,
    /// Every result request fails before the server stores it, as when the network is down.
    pub(crate) fail_report: bool,
    pub(crate) pages: HashMap<Option<String>, Page>,
    pub(crate) cursors: Vec<Option<String>>,
}

impl FakeApi {
    pub(crate) fn new(jobs: Vec<Job>) -> Self {
        Self {
            jobs: jobs.into(),
            remote: HashMap::from([(JOB_ID, job())]),
            reports: Vec::new(),
            heartbeats: Vec::new(),
            fail_report: false,
            pages: HashMap::new(),
            cursors: Vec::new(),
        }
    }

    pub(crate) fn remote(&mut self) -> &mut Job {
        self.remote.get_mut(&JOB_ID).unwrap()
    }
}

impl Holuto for FakeApi {
    fn serial(&self) -> &'static str {
        "DEMO0000001"
    }

    fn printer_id(&self) -> Option<Uuid> {
        Some(PRINTER_ID)
    }

    fn session(&mut self, _telemetry: &Telemetry) -> Result<(), ApiError> {
        Ok(())
    }

    fn heartbeat(&mut self, telemetry: &Telemetry) -> Result<(), ApiError> {
        self.heartbeats.push(telemetry.clone());
        Ok(())
    }

    fn take(&mut self) -> Result<Option<Job>, ApiError> {
        Ok(self.jobs.pop_front())
    }

    fn job(&mut self, id: Uuid) -> Result<Job, ApiError> {
        self.remote.get(&id).cloned().ok_or(ApiError::Status {
            status: 404,
            detail: None,
        })
    }

    fn report(&mut self, id: Uuid, result: &ResultForm) -> Result<(), ApiError> {
        self.reports.push(result.clone());
        if self.fail_report {
            return Err(ApiError::Response("response lost".to_owned()));
        }
        let job = self.remote.get_mut(&id).ok_or(ApiError::Status {
            status: 404,
            detail: None,
        })?;
        job.state = result.state.into();
        job.receipt_number.clone_from(&result.receipt_number);
        job.result.clone_from(&result.result);
        Ok(())
    }

    fn documents(&mut self, cursor: Option<&str>) -> Result<Page, ApiError> {
        let cursor = cursor.map(str::to_owned);
        self.cursors.push(cursor.clone());
        self.pages
            .get(&cursor)
            .cloned()
            .ok_or(ApiError::Response("page lost".to_owned()))
    }

    fn queue(&mut self, _form: &QueueForm) -> Result<(), ApiError> {
        Ok(())
    }
}
