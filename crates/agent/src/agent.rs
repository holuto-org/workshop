//! Reconciles journal evidence with Holuto without replaying printer commands.
//! Saved results are resent unchanged; terminal server states are authoritative.
//!
//! No-dispatch evidence reports FAILED only while Holuto says TAKEN. Once UNKNOWN,
//! staff must resolve it: Holuto refuses automatic UNKNOWN → FAILED.
//! Completed evidence conflicting with FAILED/EXPIRED stays blocked until an
//! operator acknowledges it locally on the same ready printer.

use std::{io, mem, time::Duration};

use fiscal::{
    device::{Capabilities, Device as PrinterDevice},
    driver::Driver,
    error::Error as PrinterError,
    journal::Journal as DeviceJournal,
    money::Grosze,
    receipt::{Advance, Line, Payment, Quantity, Receipt, VatSlot},
    report::Report,
};
use jiff::Timestamp;
use uuid::Uuid;

use i18n::Message;

use crate::{
    Error,
    api::{
        Api, ApiError,
        model::{Job, JobState, Telemetry},
    },
    journal::{
        Device, JobResult, Journal, Operation, ReceiptOperation, ReportOperation, Settlement,
        Summary,
    },
    store::StateDir,
};

pub(crate) const POLL: Duration = Duration::from_secs(3);
pub(crate) const RETRY: Duration = Duration::from_secs(15);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// Unsettled receipt; staff resolve it in Holuto.
    Receipt,
    /// Interrupted report; the operator confirms its printout on the same device.
    Report,
    /// A physical receipt conflicts with a terminal server decision.
    OrphanedReceipt,
}

struct FailureNotice {
    job_id: Uuid,
    attempt: Uuid,
    reason: Message,
}

pub(crate) struct Agent<'dir> {
    printer: Box<dyn Driver>,
    api: Api,
    journal: Journal<'dir>,
    telemetry: Telemetry,
    /// Last paired-device probe; supplies telemetry.
    status: Option<PrinterDevice>,
    /// Requests mid-operation intervention; `None` clears it.
    waiting: &'dir (dyn Fn(Option<&Message>) + Sync),
    /// Operator notices retained until drained, even if settlement is followed by an error or
    /// report.
    notices: Vec<Message>,
    /// Retained across result delivery failures; never persisted or used for recovery decisions.
    failure_notice: Option<FailureNotice>,
    /// Evidence of a printed receipt whose job Holuto failed or expired.
    orphaned_receipt: Option<Message>,
    telemetry_changed: bool,
}

impl<'dir> Agent<'dir> {
    pub(crate) fn new(
        printer: Box<dyn Driver>,
        api: Api,
        dir: &'dir StateDir,
        waiting: &'dir (dyn Fn(Option<&Message>) + Sync),
    ) -> Self {
        let telemetry = Telemetry::new(api.unique_number(), printer.vendor());
        Self {
            printer,
            api,
            journal: Journal::new(dir),
            telemetry,
            status: None,
            waiting,
            notices: Vec::new(),
            failure_notice: None,
            orphaned_receipt: None,
            telemetry_changed: true,
        }
    }

    pub(crate) const fn api(&mut self) -> &mut Api {
        &mut self.api
    }

    pub(crate) const fn status(&self) -> Option<&PrinterDevice> {
        self.status.as_ref()
    }

    pub(crate) fn take_notices(&mut self) -> Vec<Message> {
        mem::take(&mut self.notices)
    }

    /// Updates telemetry and, when the description changes, warns about active VAT rates
    /// unsupported by Holuto; polls must not repeat the warning over command results.
    pub(crate) fn describe(&mut self, device: PrinterDevice) -> Result<Vec<Message>, Error> {
        if device.unique_number != self.api.unique_number() {
            return Err(Error::Blocked(
                Message::new("agent-wrong-device")
                    .arg("connected", device.unique_number.clone())
                    .arg("paired", self.api.unique_number()),
            ));
        }
        let changed = self.status.as_ref().is_none_or(|prior| {
            prior.vendor != device.vendor
                || prior.model != device.model
                || prior.firmware != device.firmware
                || prior.vat != device.vat
                || prior.capabilities != device.capabilities
        });
        self.telemetry_changed |= changed;
        let skipped = if device.describable() {
            self.telemetry.describe(&device)
        } else {
            Vec::new()
        };
        self.status = Some(device);
        Ok(if changed { skipped } else { Vec::new() })
    }

    /// `false` means local identity/readiness cannot yet be advertised.
    pub(crate) fn session(&mut self) -> Result<bool, Error> {
        if !self.describable() {
            return Ok(false);
        }
        // `Code` constraints belong to the API, not the vendor's identity protocol.
        if [&self.telemetry.model, &self.telemetry.firmware]
            .iter()
            .any(|value| {
                !(1..=64).contains(&value.chars().count()) || value.chars().any(char::is_control)
            })
        {
            return Err(Error::Invalid(Message::new("agent-invalid-model")));
        }
        self.api.session(&self.telemetry)?;
        self.telemetry_changed = false;
        Ok(true)
    }

    pub(crate) fn refresh(&mut self) -> Result<Vec<Message>, Error> {
        let status = self.printer.probe()?;
        self.describe(status)
    }

    fn describable(&self) -> bool {
        self.status.as_ref().is_some_and(PrinterDevice::describable)
    }

    pub(crate) fn not_ready_notice(&self) -> Message {
        let description = self.status.as_ref().map_or_else(
            || Message::new("agent-no-parameters"),
            PrinterDevice::message,
        );
        Message::new("agent-not-ready").arg("description", description)
    }

    pub(crate) fn blocked(&self) -> Result<Option<Blocked>, Error> {
        Ok(self
            .journal
            .load()?
            .filter(Operation::blocks)
            .map(|operation| match operation {
                Operation::Receipt(_) if self.orphaned_receipt.is_some() => {
                    Blocked::OrphanedReceipt
                },
                Operation::Receipt(_) => Blocked::Receipt,
                Operation::Report(_) => Blocked::Report,
            }))
    }

    /// Set while `blocked` reports `Blocked::OrphanedReceipt`.
    pub(crate) const fn orphan_evidence(&self) -> Option<&Message> {
        self.orphaned_receipt.as_ref()
    }

    /// Retains qualified telemetry after a failed probe; unavailable limits stay local.
    /// Probe notices are kept at once: a later failure or block must not drop the one-time
    /// VAT warning.
    pub(crate) fn step(&mut self) -> Result<Duration, Error> {
        let probe = self.refresh().map(|notices| self.notices.extend(notices));
        if !self.describable() {
            self.notices.push(self.not_ready_notice());
            return Ok(RETRY);
        }
        if self.telemetry_changed {
            let _opened = self.session()?;
        }
        self.api.heartbeat()?;
        if let Some(blocked) = self.settle()? {
            let notice = match blocked {
                Blocked::Receipt => Message::new("agent-receipt-uncertain"),
                Blocked::Report => Message::new("agent-report-interrupted"),
                // `settle` already reported the receipt evidence the acknowledgement quotes.
                Blocked::OrphanedReceipt => return Ok(RETRY),
            };
            self.notices.push(notice);
            return Ok(RETRY);
        }
        probe?;
        if self.status.as_ref().is_some_and(|status| !status.ready()) {
            self.notices.push(self.not_ready_notice());
            return Ok(RETRY);
        }
        let Some(job) = self.api.take()? else {
            return Ok(POLL);
        };
        self.execute(&job)?;
        Ok(Duration::ZERO)
    }

    pub(crate) fn run_report(&mut self, report: Report) -> Result<(), Error> {
        let span = tracing::info_span!(
            "fiscal.report",
            sentry.op = "fiscal.report",
            command = report.label()
        );
        let _entered = span.enter();
        if let Some(blocked) = self.settle()? {
            return Err(Error::Blocked(match blocked {
                Blocked::Receipt => Message::new("agent-receipt-first"),
                // Holuto already settled the job; only the local acknowledgement remains.
                Blocked::OrphanedReceipt => Message::new("agent-orphan-first"),
                Blocked::Report => Message::new("agent-report-first"),
            }));
        }
        let unique_number = self.api.unique_number().to_owned();
        let mut operation = ReportOperation {
            report,
            vendor: self.printer.vendor(),
            device_unique_number: unique_number.clone(),
            started_at: Timestamp::now(),
            device: Device::NotStarted,
            acknowledged_at: None,
        };
        let mut dispatch = ReportDispatch {
            journal: &self.journal,
            operation: &mut operation,
            waiting: self.waiting,
        };
        if let Err(error) = self
            .printer
            .print_report(report, &unique_number, &mut dispatch)
        {
            if matches!(error, PrinterError::Device { .. })
                && matches!(operation.device, Device::Dispatching { .. })
            {
                // A bare Device error proves the driver refused this report.
                operation.device = Device::NotStarted;
                self.journal.save(&Operation::Report(operation))?;
            }
            return Err(error.into());
        }
        operation.device = Device::Completed {
            receipt_number: None,
        };
        self.journal.save(&Operation::Report(operation))
    }

    /// Confirms the printout on the same ready device; never prints.
    pub(crate) fn resolve_report(&self) -> Result<(), Error> {
        let mut report = match self.journal.load()? {
            Some(Operation::Receipt(_)) => {
                return Err(Error::Invalid(Message::new("agent-receipt-staff")));
            },
            Some(Operation::Report(report)) if report.unresolved() => report,
            Some(Operation::Report(_)) | None => {
                return Err(Error::Invalid(Message::new("agent-no-report")));
            },
        };
        if report.vendor != self.printer.vendor()
            || report.device_unique_number != self.api.unique_number()
        {
            return Err(Error::Blocked(
                Message::new("agent-report-device")
                    .arg("vendor", format!("{:?}", report.vendor))
                    .arg("number", report.device_unique_number),
            ));
        }
        let _status = self.printer.verify(&report.device_unique_number)?;
        report.acknowledged_at = Some(Timestamp::now());
        self.journal.save(&Operation::Report(report))
    }

    fn settle(&mut self) -> Result<Option<Blocked>, Error> {
        self.orphaned_receipt = None;
        let mut operation = match self.journal.load()?.filter(Operation::blocks) {
            None => return Ok(None),
            Some(Operation::Report(_)) => return Ok(Some(Blocked::Report)),
            Some(Operation::Receipt(operation)) => operation,
        };
        if operation.vendor != self.printer.vendor()
            || operation.device_unique_number != self.api.unique_number()
        {
            return Err(Error::Blocked(
                Message::new("agent-receipt-device")
                    .arg("vendor", format!("{:?}", operation.vendor))
                    .arg("number", operation.device_unique_number.clone()),
            ));
        }
        let remote = self.api.job(operation.job_id)?;
        // Binding, attempt and evidence decide settlement; job shape was judged at claim time.
        remote
            .check_printer(self.api.printer_id())
            .map_err(|problem| reconcile_error(&problem))?;
        if remote.attempt != Some(operation.attempt) {
            return Err(reconcile_error(&Message::new("agent-different-attempt")));
        }
        if let Device::Completed { receipt_number } = &operation.device
            && matches!(remote.state, JobState::Failed | JobState::Expired)
        {
            let number = receipt_number.as_ref().map_or_else(
                || Message::new("agent-unreadable-number"),
                |number| Message::literal(number.clone()),
            );
            let evidence = Message::new("agent-orphan-evidence")
                .arg("number", number)
                .arg("order", remote.order_number)
                .arg("job", remote.id.to_string())
                .arg("state", remote.state.message());
            self.notices.push(evidence.clone());
            self.orphaned_receipt = Some(evidence);
            tracing::error!(
                job_id = %remote.id, state = ?remote.state,
                "Printed receipt conflicts with terminal Holuto state"
            );
            return Ok(Some(Blocked::OrphanedReceipt));
        }
        if remote.state.terminal() {
            self.finish(&mut operation, &remote, remote.state)?;
            return Ok(None);
        }
        if matches!(operation.device, Device::NotStarted) && remote.state == JobState::Unknown {
            self.notices.push(
                Message::new("agent-no-dispatch")
                    .arg("order", remote.order_number)
                    .arg("job", remote.id.to_string()),
            );
            return Ok(Some(Blocked::Receipt));
        }
        let completed_number = match &operation.device {
            Device::Completed { receipt_number } => Some(receipt_number.clone()),
            Device::NotStarted | Device::Dispatching { .. } => None,
        };
        match (&operation.result, completed_number) {
            // A preflight failure must not clear a server UNKNOWN automatically.
            (Some(JobResult::Failed { .. }), _) if remote.state == JobState::Unknown => {
                return Ok(Some(Blocked::Receipt));
            },
            (None, Some(number)) | (Some(JobResult::Unknown { .. }), Some(number @ Some(_))) => {
                self.report(&mut operation, JobResult::done(number))?;
            },
            (None, None) => {
                let result = if matches!(operation.device, Device::NotStarted) {
                    JobResult::failed(&Message::new("agent-before-dispatch").to_string())
                } else {
                    JobResult::unknown(&Message::new("agent-interrupted").to_string())
                };
                self.report(&mut operation, result)?;
            },
            (Some(result), _) => {
                // Resend the same attempt and issue time, never the print commands.
                if !(matches!(result, JobResult::Unknown { .. })
                    && remote.state == JobState::Unknown)
                {
                    self.resend(&operation)?;
                }
            },
        }
        if matches!(operation.result, Some(JobResult::Unknown { .. })) {
            return Ok(Some(Blocked::Receipt));
        }
        let state = operation
            .result
            .as_ref()
            .map_or(JobState::Unknown, |result| result.outcome().into());
        self.finish(&mut operation, &remote, state)?;
        Ok(None)
    }

    fn execute(&mut self, job: &Job) -> Result<(), Error> {
        let span = tracing::info_span!(
            "fiscal.receipt", sentry.op = "fiscal.receipt",
            job_id = %job.id, attempt = ?job.attempt, kind = ?job.kind
        );
        let _entered = span.enter();
        tracing::info!(job_id = %job.id, "Job claimed");
        job.check_printer(self.api.printer_id())
            .map_err(|problem| invalid_job(job.id, &problem))?;
        let (JobState::Taken, Some(attempt)) = (job.state, job.attempt) else {
            return Err(invalid_job(job.id, &Message::new("agent-not-taken")));
        };
        match self.journal.load()? {
            Some(Operation::Receipt(previous)) if previous.job_id == job.id => {
                return Err(Error::Blocked(
                    Message::new("agent-already-executed").arg("job", job.id.to_string()),
                ));
            },
            Some(previous) if previous.blocks() => {
                return Err(Error::Blocked(Message::new("agent-previous-unresolved")));
            },
            Some(_) | None => {},
        }
        let mut operation = ReceiptOperation {
            job_id: job.id,
            attempt,
            issued_at: Timestamp::now(),
            vendor: self.printer.vendor(),
            device_unique_number: self.api.unique_number().to_owned(),
            device: Device::NotStarted,
            summary: None,
            result: None,
            settled: None,
        };
        // Persist before any physical action; interruptions become UNKNOWN, never retries.
        self.journal.save(&Operation::Receipt(operation.clone()))?;
        let status = match self
            .printer
            .probe()
            .map_err(Error::from)
            .and_then(|status| {
                let notices = self.describe(status.clone())?;
                self.notices.extend(notices);
                Ok(status)
            }) {
            Ok(status) => status,
            Err(error) => {
                return self.fail(&mut operation, job, error.message());
            },
        };
        let receipt = match self.receipt(job, &status.capabilities) {
            Ok(receipt) => receipt,
            Err(reason) => {
                return self.fail(&mut operation, job, reason);
            },
        };
        operation.summary = Some(Summary::of(&receipt));
        let mut dispatch = ReceiptDispatch {
            journal: &self.journal,
            operation: &mut operation,
            waiting: self.waiting,
        };
        let printed = self.printer.print_receipt(&receipt, &status, &mut dispatch);
        match printed {
            Ok(printed) => {
                operation.device = Device::Completed {
                    receipt_number: printed.receipt_number.clone(),
                };
                operation.issued_at = printed.issued_at;
                self.journal.save(&Operation::Receipt(operation.clone()))?;
                tracing::info!(job_id = %job.id, "Receipt printed");
                self.report(&mut operation, JobResult::done(printed.receipt_number))?;
                if matches!(operation.result, Some(JobResult::Done { .. })) {
                    self.finish(&mut operation, job, JobState::Done)?;
                }
                Ok(())
            },
            Err(error @ PrinterError::Uncertain(_)) => {
                tracing::error!(job_id = %job.id, "Fiscal receipt outcome is uncertain");
                self.report(&mut operation, JobResult::unknown(&error.to_string()))
            },
            Err(error) => self.fail(&mut operation, job, error.message()),
        }
    }

    fn receipt(&self, job: &Job, capabilities: &Capabilities) -> Result<Receipt, Message> {
        job.check(capabilities.max_lines)
            .map_err(|problem| Message::new("agent-invalid-job-shape").arg("problem", problem))?;
        if !job.advances.is_empty() && !capabilities.advance_deductions {
            return Err(Message::new("agent-unsupported-advances"));
        }
        let rates = &self.telemetry.vat_rates;
        let slot_rate = |slot: u8, rate| {
            rates
                .iter()
                .any(|known| known.letter.slot() == slot && known.vat_rate == rate)
        };
        let mismatch = job
            .lines
            .iter()
            .map(|line| (line.vat_letter.slot(), line.vat_rate))
            .chain(
                job.advances
                    .iter()
                    .map(|advance| (advance.vat_letter.slot(), advance.vat_rate)),
            )
            .find(|(slot, rate)| !slot_rate(*slot, *rate));
        if let Some((slot, rate)) = mismatch {
            return Err(Message::new("agent-vat-mismatch")
                .arg("rate", rate.label())
                .arg("slot", slot));
        }
        let mut lines = Vec::with_capacity(job.lines.len());
        for item in &job.lines {
            let line = Line::new(
                &item.name,
                quantity(&item.quantity)?,
                amount(Message::new("agent-field-price"), &item.unit_price)?,
                slot(item.vat_letter.slot())?,
                capabilities,
            )
            .map_err(|error| error.message())?;
            if line.total() != amount(Message::new("agent-field-line-total"), &item.total)? {
                return Err(Message::new("agent-line-total"));
            }
            lines.push(line);
        }
        if lines.iter().map(Line::total).sum::<Grosze>()
            != amount(Message::new("agent-field-job-total"), &job.total)?
        {
            return Err(Message::new("agent-job-total"));
        }
        let advances = job
            .advances
            .iter()
            .map(|advance| {
                Ok(Advance {
                    vat: slot(advance.vat_letter.slot())?,
                    amount: amount(Message::new("agent-field-advance"), &advance.amount)?,
                })
            })
            .collect::<Result<Vec<_>, Message>>()?;
        let payments = job
            .payments
            .iter()
            .map(|payment| {
                Ok(Payment {
                    form: payment.form,
                    amount: amount(Message::new("agent-field-payment"), &payment.amount)?,
                })
            })
            .collect::<Result<Vec<_>, Message>>()?;
        let receipt = Receipt::new(
            lines,
            advances,
            payments,
            job.buyer_nip.clone(),
            &job.company_name,
            capabilities,
        )
        .map_err(|error| error.message())?;
        if receipt.fiscal_total() == Grosze::ZERO && !capabilities.zero_total_receipts {
            return Err(Message::new("agent-unsupported-zero"));
        }
        Ok(receipt)
    }

    fn fail(
        &mut self,
        operation: &mut ReceiptOperation,
        job: &Job,
        reason: Message,
    ) -> Result<(), Error> {
        let result = JobResult::failed(&reason.to_string());
        self.failure_notice = Some(FailureNotice {
            job_id: operation.job_id,
            attempt: operation.attempt,
            reason,
        });
        self.report(operation, result)?;
        self.finish(operation, job, JobState::Failed)
    }

    fn report(&mut self, operation: &mut ReceiptOperation, result: JobResult) -> Result<(), Error> {
        operation.result = Some(result);
        self.journal.save(&Operation::Receipt(operation.clone()))?;
        self.resend(operation)
    }

    fn resend(&mut self, operation: &ReceiptOperation) -> Result<(), Error> {
        let Some(form) = operation.result_form() else {
            return Ok(());
        };
        match self.api.report(operation.job_id, &form) {
            Ok(()) => Ok(()),
            Err(error @ ApiError::Status { status: 400, .. })
                if matches!(operation.device, Device::Completed { .. }) =>
            {
                self.notices.push(
                    Message::new("agent-rejected-evidence")
                        .arg("job", operation.job_id.to_string()),
                );
                tracing::error!(
                    job_id = %operation.job_id, "Holuto rejected completed fiscal evidence"
                );
                Err(error.into())
            },
            Err(error) => Err(error.into()),
        }
    }

    fn finish(
        &mut self,
        operation: &mut ReceiptOperation,
        job: &Job,
        state: JobState,
    ) -> Result<(), Error> {
        self.orphaned_receipt = None;
        if state == JobState::Failed
            && !matches!(operation.device, Device::Completed { .. })
            && let Some(reason) = operation
                .result
                .as_ref()
                .and_then(|result| match result {
                    JobResult::Failed { reason } | JobResult::Unknown { reason } => Some(reason),
                    JobResult::Done { .. } => None,
                })
                .or(job.result.as_ref())
        {
            let reason = self
                .failure_notice
                .as_ref()
                .filter(|notice| {
                    notice.job_id == operation.job_id && notice.attempt == operation.attempt
                })
                .map_or_else(
                    || Message::literal(reason.clone()),
                    |notice| notice.reason.clone(),
                );
            self.notices.push(
                Message::new("agent-not-printed")
                    .arg("order", job.order_number)
                    .arg("reason", reason),
            );
        }
        operation.settled = Some(Settlement {
            at: Timestamp::now(),
            state,
        });
        self.journal.save(&Operation::Receipt(operation.clone()))?;
        self.failure_notice = None;
        Ok(())
    }

    /// Explicit local acknowledgement of evidence Holuto cannot accept.
    /// Same ready device is required; this never sends a fiscal command or result.
    pub(crate) fn acknowledge_orphaned_receipt(&mut self) -> Result<(), Error> {
        let Some(Operation::Receipt(mut operation)) = self.journal.load()? else {
            return Err(Error::Invalid(Message::new("agent-no-receipt")));
        };
        if operation.settled.is_some() || !matches!(operation.device, Device::Completed { .. }) {
            return Err(Error::Invalid(Message::new("agent-no-printed-receipt")));
        }
        if operation.vendor != self.printer.vendor()
            || operation.device_unique_number != self.api.unique_number()
        {
            return Err(Error::Blocked(
                Message::new("agent-orphan-device")
                    .arg("vendor", format!("{:?}", operation.vendor))
                    .arg("number", operation.device_unique_number.clone()),
            ));
        }
        let _status = self.printer.verify(&operation.device_unique_number)?;
        let remote = self.api.job(operation.job_id)?;
        remote
            .check_printer(self.api.printer_id())
            .map_err(|problem| reconcile_error(&problem))?;
        if remote.attempt != Some(operation.attempt)
            || !matches!(remote.state, JobState::Failed | JobState::Expired)
        {
            return Err(reconcile_error(&Message::new("agent-state-no-resolution")));
        }
        tracing::info!(
            job_id = %operation.job_id, state = ?remote.state,
            "Operator acknowledged orphaned receipt; no command reissued"
        );
        self.finish(&mut operation, &remote, remote.state)
    }
}

struct ReceiptDispatch<'a> {
    journal: &'a Journal<'a>,
    operation: &'a mut ReceiptOperation,
    waiting: &'a dyn Fn(Option<&Message>),
}

impl DeviceJournal for ReceiptDispatch<'_> {
    fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
        let mut next = self.operation.clone();
        next.device = Device::Dispatching {
            stage: command.to_owned(),
        };
        self.journal.write(&Operation::Receipt(next.clone()))?;
        *self.operation = next;
        Ok(())
    }

    fn waiting(&mut self, problem: Option<&Message>) {
        (self.waiting)(problem);
    }
}

/// Reports enter the journal only when the first command is about to be sent.
struct ReportDispatch<'a> {
    journal: &'a Journal<'a>,
    operation: &'a mut ReportOperation,
    waiting: &'a dyn Fn(Option<&Message>),
}

impl DeviceJournal for ReportDispatch<'_> {
    fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
        let mut next = self.operation.clone();
        next.device = Device::Dispatching {
            stage: command.to_owned(),
        };
        self.journal.write(&Operation::Report(next.clone()))?;
        *self.operation = next;
        Ok(())
    }

    fn waiting(&mut self, problem: Option<&Message>) {
        (self.waiting)(problem);
    }
}

fn quantity(text: &str) -> Result<Quantity, Message> {
    Quantity::parse(text).map_err(|error| {
        Message::new("agent-quantity-detail")
            .arg("error", error.message())
            .arg("text", format!("{text:?}"))
    })
}

fn amount(field: Message, text: &str) -> Result<Grosze, Message> {
    Grosze::parse(text).map_err(|error| {
        Message::new("agent-amount-detail")
            .arg("field", field)
            .arg("error", error.message())
            .arg("text", format!("{text:?}"))
    })
}

fn slot(slot: u8) -> Result<VatSlot, Message> {
    VatSlot::new(slot).map_err(|error| error.message())
}

fn invalid_job(id: Uuid, problem: &Message) -> Error {
    Error::Blocked(
        Message::new("agent-invalid-job")
            .arg("job", id.to_string())
            .arg("problem", problem.clone()),
    )
}

fn reconcile_error(problem: &Message) -> Error {
    Error::Blocked(Message::new("agent-reconcile").arg("problem", problem.clone()))
}

#[cfg(test)]
mod tests;
