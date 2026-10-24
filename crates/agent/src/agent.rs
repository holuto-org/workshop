//! Executes Holuto jobs on the printer and settles their results, durably and without replay.
//!
//! A receipt job moves through the journal as: claimed (`NotStarted`, before preflight), one
//! `Dispatching` entry per printer command, `Completed`, then the result persisted and sent, then
//! settled. After a restart the journal decides: nothing is ever sent to the printer again; an
//! unsettled receipt is reconciled with the job's server state and its saved result is resent
//! unchanged. A terminal server state (staff resolution) is authoritative.

use std::{io, time::Duration};

use jiff::Timestamp;
use posnet::{
    money::Grosze,
    printer::{Journal as DeviceJournal, Printer, Report, Status},
    receipt::{Advance, Line, Payment, Receipt, VatSlot},
};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    Error,
    api::{
        Holuto,
        model::{Job, JobState, Telemetry, printer_slots},
    },
    history::{self, HistoryEntry},
    journal::{
        Device, JobResult, Journal, Operation, ReceiptOperation, ReportOperation, Settlement,
        Summary,
    },
    store::StateDir,
};

pub(crate) const POLL: Duration = Duration::from_secs(3);
pub(crate) const RETRY: Duration = Duration::from_secs(15);
const INTERRUPTED: &str = "Przerwano w trakcie drukowania. Sprawdź drukarkę.";
const UNNUMBERED: &str = "Wydruk zakończony, ale nie odczytano numeru paragonu. Sprawdź wydruk i \
                          rozstrzygnij zadanie w Holuto.";

/// What one poll did and when the next one should run.
#[derive(Debug, Default)]
pub(crate) struct Step {
    pub(crate) delay: Duration,
    /// Warnings for the operator; none of them stop processing.
    pub(crate) notices: Vec<String>,
    pub(crate) history_changed: bool,
}

/// Why new work is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// A receipt result is not settled with Holuto; staff resolve it in the web app.
    Receipt,
    /// A report was interrupted; an operator confirms the printout on the same device.
    Report,
}

#[derive(Debug)]
pub(crate) struct Agent<'dir, H> {
    printer: Printer,
    api: H,
    dir: &'dir StateDir,
    journal: Journal,
    telemetry: Telemetry,
    /// The status the telemetry describes: the paired device as last probed.
    status: Option<Status>,
}

impl<'dir, H: Holuto> Agent<'dir, H> {
    pub(crate) fn new(printer: Printer, api: H, dir: &'dir StateDir) -> Self {
        let telemetry = Telemetry::new(api.serial());
        Self {
            printer,
            api,
            dir,
            journal: Journal::new(dir),
            telemetry,
            status: None,
        }
    }

    pub(crate) const fn api(&mut self) -> &mut H {
        &mut self.api
    }

    pub(crate) const fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Adopts a fresh probe of the paired device into the telemetry. Returns notices for active
    /// slots whose rate Holuto does not use.
    pub(crate) fn describe(&mut self, status: Status) -> Result<Vec<String>, Error> {
        if status.serial != self.api.serial() {
            return Err(Error::Blocked(format!(
                "Podłączona drukarka {} nie jest drukarką {} sparowaną przy uruchomieniu. \
                 Podłącz ją ponownie albo zatrzymaj i uruchom obsługę.",
                status.serial,
                self.api.serial()
            )));
        }
        let (slots, skipped) = printer_slots(&status.vat);
        status.name.clone_into(&mut self.telemetry.model);
        status.version.clone_into(&mut self.telemetry.firmware);
        self.telemetry.vat_slots = slots;
        self.status = Some(status);
        Ok(skipped)
    }

    pub(crate) fn session(&mut self) -> Result<(), Error> {
        let _printer = self.api.session(&self.telemetry)?;
        Ok(())
    }

    /// Whether new work is refused, and why.
    pub(crate) fn blocked(&self) -> Result<Option<Blocked>, Error> {
        Ok(self.journal.load()?.and_then(|operation| match operation {
            Operation::Receipt(receipt) => receipt.settled.is_none().then_some(Blocked::Receipt),
            Operation::Report(report) => report.unresolved().then_some(Blocked::Report),
        }))
    }

    /// One poll: probe, heartbeat (even when the probe failed, so Holuto sees the device alive),
    /// settle the last operation, and take and execute one job when nothing blocks.
    pub(crate) fn step(&mut self) -> Result<Step, Error> {
        let mut step = Step {
            delay: POLL,
            ..Step::default()
        };
        let probe = self
            .printer
            .probe()
            .map_err(Error::from)
            .and_then(|status| self.describe(status));
        let _printer = self.api.heartbeat(&self.telemetry)?;
        let blocked = self.settle(&mut step)?;
        match blocked {
            Some(Blocked::Receipt) => {
                step.notices.push(
                    "Niepotwierdzony wynik. Sprawdź wydruk i rozstrzygnij zadanie w Holuto."
                        .to_owned(),
                );
                step.delay = RETRY;
                return Ok(step);
            },
            Some(Blocked::Report) => {
                step.notices.push(
                    "Przerwany raport. Sprawdź wydruk i kliknij „Wyjaśnij raport”.".to_owned(),
                );
                step.delay = RETRY;
                return Ok(step);
            },
            None => {},
        }
        step.notices.extend(probe?);
        if let Some(status) = self.status.as_ref().filter(|status| !status.ready()) {
            step.notices
                .push(format!("Drukarka niegotowa: {}", status.description()));
            step.delay = RETRY;
            return Ok(step);
        }
        let Some(job) = self.api.take()? else {
            return Ok(step);
        };
        self.execute(&job, &mut step)?;
        step.delay = Duration::ZERO;
        Ok(step)
    }

    /// Prints a fiscal report. Refused while any operation is unresolved.
    pub(crate) fn run_report(&mut self, report: Report) -> Result<(), Error> {
        let mut step = Step::default();
        match self.settle(&mut step)? {
            Some(Blocked::Receipt) => {
                return Err(Error::Blocked(
                    "Najpierw rozstrzygnij niepotwierdzony paragon w Holuto.".to_owned(),
                ));
            },
            Some(Blocked::Report) => {
                return Err(Error::Blocked(
                    "Najpierw wyjaśnij przerwany raport.".to_owned(),
                ));
            },
            None => {},
        }
        let serial = self.api.serial().to_owned();
        let mut operation = ReportOperation {
            report,
            device_serial: serial.clone(),
            started_at: Timestamp::now(),
            device: Device::NotStarted,
            acknowledged_at: None,
        };
        let mut dispatch = ReportDispatch {
            journal: &self.journal,
            operation: &mut operation,
        };
        self.printer.print_report(report, &serial, &mut dispatch)?;
        operation.device = Device::Completed {
            receipt_number: None,
        };
        self.journal.save(&Operation::Report(operation))
    }

    /// Records that an operator checked the printout of an interrupted report.
    ///
    /// The device it ran on must be connected and ready, so the check was possible. Never prints.
    pub(crate) fn resolve_report(&self) -> Result<(), Error> {
        let Some(operation) = self.journal.load()? else {
            return Err(Error::Invalid(
                "Brak przerwanego raportu do wyjaśnienia.".to_owned(),
            ));
        };
        let mut report = match operation {
            Operation::Receipt(_) => {
                return Err(Error::Invalid(
                    "Wynik paragonu rozstrzyga personel w aplikacji Holuto.".to_owned(),
                ));
            },
            Operation::Report(report) if report.unresolved() => report,
            Operation::Report(_) => {
                return Err(Error::Invalid(
                    "Brak przerwanego raportu do wyjaśnienia.".to_owned(),
                ));
            },
        };
        let _status = self.printer.verify(&report.device_serial)?;
        report.acknowledged_at = Some(Timestamp::now());
        self.journal.save(&Operation::Report(report))
    }

    /// Reconciles the journaled operation; `Some` while it still blocks new work.
    fn settle(&mut self, step: &mut Step) -> Result<Option<Blocked>, Error> {
        let mut operation = match self.journal.load()? {
            None => return Ok(None),
            Some(Operation::Report(report)) => {
                return Ok(report.unresolved().then_some(Blocked::Report));
            },
            Some(Operation::Receipt(operation)) if operation.settled.is_some() => return Ok(None),
            Some(Operation::Receipt(operation)) => operation,
        };
        if operation.device_serial != self.api.serial() {
            return Err(Error::Blocked(format!(
                "Niepotwierdzony paragon dotyczy drukarki {}. Podłącz ją, aby uzgodnić wynik z \
                 Holuto.",
                operation.device_serial
            )));
        }
        let remote = self.api.job(operation.job_id)?;
        remote
            .check(self.api.printer_id())
            .map_err(|problem| reconcile_error(&problem))?;
        if remote.attempt != Some(operation.attempt) {
            return Err(reconcile_error(
                "Holuto zna inną próbę wykonania tego zadania",
            ));
        }
        if remote.state.terminal() {
            self.finish(
                &mut operation,
                remote.state,
                remote.receipt_number.as_deref(),
                step,
            )?;
            return Ok(None);
        }
        let completed_number = match &operation.device {
            Device::Completed { receipt_number } => Some(receipt_number.clone()),
            Device::NotStarted | Device::Dispatching { .. } => None,
        };
        match (&operation.result, completed_number) {
            // Even a known preflight failure never clears a server UNKNOWN automatically.
            (Some(JobResult::Failed { .. }), _) if remote.state == JobState::Unknown => {
                return Ok(Some(Blocked::Receipt));
            },
            // Completed on the device: report it, upgrading an earlier UNKNOWN once numbered.
            (None, Some(number)) | (Some(JobResult::Unknown { .. }), Some(number @ Some(_))) => {
                self.report(&mut operation, done(number))?;
            },
            (None, None) => self.report(&mut operation, JobResult::unknown(INTERRUPTED))?,
            (Some(result), _) => {
                // Resending a saved result is not reprinting: same attempt, same issue time.
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
        self.finish(&mut operation, state, None, step)?;
        Ok(None)
    }

    fn execute(&mut self, job: &Job, step: &mut Step) -> Result<(), Error> {
        job.check(self.api.printer_id())
            .map_err(|problem| invalid_job(job.id, &problem))?;
        let (JobState::Taken, Some(attempt)) = (job.state, job.attempt) else {
            return Err(invalid_job(
                job.id,
                "zadanie nie zostało pobrane do wykonania",
            ));
        };
        match self.journal.load()? {
            Some(Operation::Receipt(previous)) if previous.job_id == job.id => {
                return Err(Error::Blocked(format!(
                    "Zadanie {} było już wykonywane na tej drukarce; nie zostanie wydrukowane \
                     ponownie.",
                    job.id
                )));
            },
            Some(previous) if previous.blocks() => {
                return Err(Error::Blocked(
                    "Poprzednia operacja jest nierozstrzygnięta.".to_owned(),
                ));
            },
            Some(_) | None => {},
        }
        let mut operation = ReceiptOperation {
            job_id: job.id,
            attempt,
            issued_at: Timestamp::now(),
            device_serial: self.api.serial().to_owned(),
            device: Device::NotStarted,
            summary: None,
            result: None,
            settled: None,
        };
        // The claim is durable before any possible physical action: an interruption from here on
        // is reported as UNKNOWN, never retried.
        self.journal.save(&Operation::Receipt(operation.clone()))?;
        let status = self.printer.probe()?;
        step.notices.extend(self.describe(status.clone())?);
        let receipt = match self.receipt(job) {
            Ok(receipt) => receipt,
            Err(reason) => {
                self.report(&mut operation, JobResult::failed(&reason))?;
                return self.finish(&mut operation, JobState::Failed, None, step);
            },
        };
        operation.summary = Some(Summary::of(&receipt));
        let mut dispatch = ReceiptDispatch {
            journal: &self.journal,
            operation: &mut operation,
        };
        let printed = self.printer.print_receipt(&receipt, &status, &mut dispatch);
        match printed {
            Ok(printed) => {
                operation.device = Device::Completed {
                    receipt_number: printed.receipt_number.clone(),
                };
                self.journal.save(&Operation::Receipt(operation.clone()))?;
                self.report(&mut operation, done(printed.receipt_number))?;
                if matches!(operation.result, Some(JobResult::Done { .. })) {
                    self.finish(&mut operation, JobState::Done, None, step)?;
                }
                Ok(())
            },
            // Only an uncertain error means a command reached the printer.
            Err(error @ posnet::Error::Uncertain(_)) => {
                self.report(&mut operation, JobResult::unknown(&error.to_string()))
            },
            Err(error) => {
                self.report(&mut operation, JobResult::failed(&error.to_string()))?;
                self.finish(&mut operation, JobState::Failed, None, step)
            },
        }
    }

    /// The job as a printer receipt; `Err` is the reason the job fails before printing.
    fn receipt(&self, job: &Job) -> Result<Receipt, String> {
        let rates = &self.telemetry.vat_slots;
        let slot_rate = |slot: u8, rate| {
            rates
                .iter()
                .any(|known| known.slot == slot && known.vat_rate == rate)
        };
        let mismatch = job
            .lines
            .iter()
            .map(|line| (line.vat_slot, line.vat_rate))
            .chain(
                job.advances
                    .iter()
                    .map(|advance| (advance.vat_slot, advance.vat_rate)),
            )
            .find(|(slot, rate)| !slot_rate(*slot, *rate));
        if let Some((slot, rate)) = mismatch {
            return Err(format!(
                "Stawka VAT {} zadania nie odpowiada slotowi {slot} drukarki. Odśwież stawki w \
                 Holuto.",
                rate.label()
            ));
        }
        let mut lines = Vec::with_capacity(job.lines.len());
        for item in &job.lines {
            let line = Line::new(
                &item.name,
                decimal(&item.quantity)?,
                decimal(&item.unit_price)?,
                slot(item.vat_slot)?,
            )
            .map_err(|error| error.to_string())?;
            if line.total() != grosze(&item.total)? {
                return Err(
                    "Wartość pozycji nie zgadza się z zaokrąglonym iloczynem ilości i ceny."
                        .to_owned(),
                );
            }
            lines.push(line);
        }
        if lines.iter().map(Line::total).sum::<Grosze>() != grosze(&job.total)? {
            return Err("Suma pozycji nie odpowiada wartości zadania.".to_owned());
        }
        let advances = job
            .advances
            .iter()
            .map(|advance| {
                Ok(Advance {
                    vat: slot(advance.vat_slot)?,
                    amount: grosze(&advance.amount)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let payments = job
            .payments
            .iter()
            .map(|payment| {
                Ok(Payment {
                    form: payment.form,
                    amount: grosze(&payment.amount)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Receipt::new(
            lines,
            advances,
            payments,
            job.buyer_nip.clone(),
            &job.company_name,
        )
        .map_err(|error| error.to_string())
    }

    /// Persists the result, then sends it.
    fn report(&mut self, operation: &mut ReceiptOperation, result: JobResult) -> Result<(), Error> {
        operation.result = Some(result);
        self.journal.save(&Operation::Receipt(operation.clone()))?;
        self.resend(operation)
    }

    fn resend(&mut self, operation: &ReceiptOperation) -> Result<(), Error> {
        let Some(form) = operation.result_form() else {
            return Ok(());
        };
        let _job = self.api.report(operation.job_id, &form)?;
        Ok(())
    }

    /// Settles the operation with the backend `state`; a printed receipt enters the history, with
    /// the number staff supplied in Holuto when the printer's could not be read.
    fn finish(
        &self,
        operation: &mut ReceiptOperation,
        state: JobState,
        staff_number: Option<&str>,
        step: &mut Step,
    ) -> Result<(), Error> {
        if let (Device::Completed { receipt_number }, Some(summary)) =
            (&operation.device, &operation.summary)
        {
            let entry = HistoryEntry {
                job_id: operation.job_id,
                printed_at: operation.issued_at,
                receipt_number: receipt_number
                    .clone()
                    .or_else(|| staff_number.map(str::to_owned)),
                total: summary.total,
                payments: summary
                    .payments
                    .iter()
                    .map(|payment| (payment.form, payment.amount))
                    .collect(),
                items: summary.items.clone(),
            };
            match history::append(self.dir, &entry) {
                Ok(()) => step.history_changed = true,
                Err(error) => step.notices.push(error.to_string()),
            }
        }
        operation.settled = Some(Settlement {
            at: Timestamp::now(),
            state,
        });
        self.journal.save(&Operation::Receipt(operation.clone()))
    }
}

/// Journals each receipt command before it is sent.
struct ReceiptDispatch<'a> {
    journal: &'a Journal,
    operation: &'a mut ReceiptOperation,
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
}

/// Journals each report command before it is sent; the report is journaled only once a command
/// is about to reach the printer.
struct ReportDispatch<'a> {
    journal: &'a Journal,
    operation: &'a mut ReportOperation,
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
}

/// A printed receipt without a readable number stays UNKNOWN until staff supply the number.
fn done(receipt_number: Option<String>) -> JobResult {
    receipt_number.map_or_else(
        || JobResult::unknown(UNNUMBERED),
        |receipt_number| JobResult::Done { receipt_number },
    )
}

fn decimal(text: &str) -> Result<Decimal, String> {
    text.parse()
        .map_err(|_unparsable| format!("Nieprawidłowa liczba w zadaniu: {text:?}."))
}

fn grosze(text: &str) -> Result<Grosze, String> {
    Grosze::from_zloty(decimal(text)?).map_err(|error| format!("{error} ({text})"))
}

fn slot(slot: u8) -> Result<VatSlot, String> {
    VatSlot::new(slot).map_err(|error| error.to_string())
}

fn invalid_job(id: Uuid, problem: &str) -> Error {
    Error::Blocked(format!(
        "Nieprawidłowe zadanie {id} z Holuto: {problem}. Nie drukuję go."
    ))
}

fn reconcile_error(problem: &str) -> Error {
    Error::Blocked(format!(
        "Nie można uzgodnić niepotwierdzonego paragonu z Holuto: {problem}. Drukowanie zablokowane."
    ))
}

#[cfg(test)]
mod tests;
