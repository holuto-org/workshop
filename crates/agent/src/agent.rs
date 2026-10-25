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

use crate::{
    Error,
    api::{
        Api, ApiError,
        model::{Job, JobState, Telemetry},
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

/// Retained until drained, even if settlement is followed by an error or report.
#[derive(Default)]
pub(crate) struct News {
    pub(crate) notices: Vec<String>,
    pub(crate) history: Option<Vec<HistoryEntry>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// Unsettled receipt; staff resolve it in Holuto.
    Receipt,
    /// Interrupted report; the operator confirms its printout on the same device.
    Report,
    /// A physical receipt conflicts with a terminal server decision.
    OrphanedReceipt,
}

pub(crate) struct Agent<'dir> {
    printer: Box<dyn Driver>,
    api: Api,
    dir: &'dir StateDir,
    journal: Journal<'dir>,
    telemetry: Telemetry,
    /// Last paired-device probe; supplies telemetry.
    status: Option<PrinterDevice>,
    /// Requests mid-operation intervention; `None` clears it.
    waiting: &'dir (dyn Fn(Option<&str>) + Sync),
    news: News,
    orphaned_receipt: bool,
    telemetry_changed: bool,
}

impl<'dir> Agent<'dir> {
    pub(crate) fn new(
        printer: Box<dyn Driver>,
        api: Api,
        dir: &'dir StateDir,
        waiting: &'dir (dyn Fn(Option<&str>) + Sync),
    ) -> Self {
        let telemetry = Telemetry::new(api.unique_number(), printer.vendor());
        Self {
            printer,
            api,
            dir,
            journal: Journal::new(dir),
            telemetry,
            status: None,
            waiting,
            news: News::default(),
            orphaned_receipt: false,
            telemetry_changed: true,
        }
    }

    pub(crate) const fn api(&mut self) -> &mut Api {
        &mut self.api
    }

    pub(crate) const fn status(&self) -> Option<&PrinterDevice> {
        self.status.as_ref()
    }

    pub(crate) fn take_news(&mut self) -> News {
        mem::take(&mut self.news)
    }

    /// Updates telemetry and warns about active VAT rates unsupported by Holuto.
    pub(crate) fn describe(&mut self, device: PrinterDevice) -> Result<Vec<String>, Error> {
        if device.unique_number != self.api.unique_number() {
            return Err(Error::Blocked(format!(
                "Podłączona drukarka {} nie jest drukarką {} sparowaną przy uruchomieniu. \
                 Podłącz ją ponownie albo zatrzymaj i uruchom obsługę.",
                device.unique_number,
                self.api.unique_number()
            )));
        }
        self.telemetry_changed |= self.status.as_ref().is_none_or(|prior| {
            prior.vendor != device.vendor
                || prior.model != device.model
                || prior.firmware != device.firmware
                || prior.vat != device.vat
                || prior.capabilities != device.capabilities
        });
        let skipped = if device.describable_capabilities().is_some() {
            self.telemetry.describe(&device)
        } else {
            Vec::new()
        };
        self.status = Some(device);
        Ok(skipped)
    }

    /// `false` means local identity/readiness cannot yet be advertised.
    pub(crate) fn session(&mut self) -> Result<bool, Error> {
        if !self.describable() {
            return Ok(false);
        }
        self.api.session(&self.telemetry)?;
        self.telemetry_changed = false;
        Ok(true)
    }

    pub(crate) fn refresh(&mut self) -> Result<Vec<String>, Error> {
        let status = self.printer.probe()?;
        self.describe(status)
    }

    fn describable(&self) -> bool {
        self.status
            .as_ref()
            .and_then(PrinterDevice::describable_capabilities)
            .is_some()
    }

    pub(crate) fn not_ready_notice(&self) -> String {
        let description = self.status.as_ref().map_or_else(
            || "Nie odczytano parametrów drukarki".to_owned(),
            PrinterDevice::description,
        );
        format!("Drukarka niegotowa: {description}")
    }

    pub(crate) fn blocked(&self) -> Result<Option<Blocked>, Error> {
        Ok(self
            .journal
            .load()?
            .filter(Operation::blocks)
            .map(|operation| match operation {
                Operation::Receipt(_) if self.orphaned_receipt => Blocked::OrphanedReceipt,
                Operation::Receipt(_) => Blocked::Receipt,
                Operation::Report(_) => Blocked::Report,
            }))
    }

    /// Retains qualified telemetry after a failed probe; unavailable limits stay local.
    pub(crate) fn step(&mut self) -> Result<Duration, Error> {
        let probe = self.refresh();
        if !self.describable() {
            self.news.notices.push(self.not_ready_notice());
            return Ok(RETRY);
        }
        if self.telemetry_changed {
            let _opened = self.session()?;
        }
        self.api.heartbeat()?;
        if let Some(blocked) = self.settle()? {
            self.news.notices.push(
                match blocked {
                    Blocked::Receipt | Blocked::OrphanedReceipt => {
                        "Niepotwierdzony wynik. Sprawdź wydruk i rozstrzygnij zadanie w Holuto."
                    },
                    Blocked::Report => {
                        "Przerwany raport. Sprawdź wydruk i kliknij „Wyjaśnij raport”."
                    },
                }
                .to_owned(),
            );
            return Ok(RETRY);
        }
        self.news.notices.extend(probe?);
        if self.status.as_ref().is_some_and(|status| !status.ready()) {
            self.news.notices.push(self.not_ready_notice());
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
            return Err(Error::Blocked(
                match blocked {
                    Blocked::Receipt | Blocked::OrphanedReceipt => {
                        "Najpierw rozstrzygnij niepotwierdzony paragon w Holuto."
                    },
                    Blocked::Report => "Najpierw wyjaśnij przerwany raport.",
                }
                .to_owned(),
            ));
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
                return Err(Error::Invalid(
                    "Wynik paragonu rozstrzyga personel w aplikacji Holuto.".to_owned(),
                ));
            },
            Some(Operation::Report(report)) if report.unresolved() => report,
            Some(Operation::Report(_)) | None => {
                return Err(Error::Invalid(
                    "Brak przerwanego raportu do wyjaśnienia.".to_owned(),
                ));
            },
        };
        if report.vendor != self.printer.vendor()
            || report.device_unique_number != self.api.unique_number()
        {
            return Err(Error::Blocked(format!(
                "Podłącz drukarkę {:?} {}, której dotyczy przerwany raport.",
                report.vendor, report.device_unique_number
            )));
        }
        let _status = self.printer.verify(&report.device_unique_number)?;
        report.acknowledged_at = Some(Timestamp::now());
        self.journal.save(&Operation::Report(report))
    }

    fn settle(&mut self) -> Result<Option<Blocked>, Error> {
        self.orphaned_receipt = false;
        let mut operation = match self.journal.load()?.filter(Operation::blocks) {
            None => return Ok(None),
            Some(Operation::Report(_)) => return Ok(Some(Blocked::Report)),
            Some(Operation::Receipt(operation)) => operation,
        };
        if operation.vendor != self.printer.vendor()
            || operation.device_unique_number != self.api.unique_number()
        {
            return Err(Error::Blocked(format!(
                "Niepotwierdzony paragon dotyczy drukarki {:?} {}. Podłącz ją, aby uzgodnić wynik \
                 z Holuto.",
                operation.vendor, operation.device_unique_number
            )));
        }
        let remote = self.api.job(operation.job_id)?;
        remote
            .check(self.api.printer_id(), None)
            .map_err(|problem| reconcile_error(&problem))?;
        if remote.attempt != Some(operation.attempt) {
            return Err(reconcile_error(
                "Holuto zna inną próbę wykonania tego zadania",
            ));
        }
        if let Device::Completed { receipt_number } = &operation.device
            && matches!(remote.state, JobState::Failed | JobState::Expired)
        {
            self.orphaned_receipt = true;
            let number = receipt_number.as_deref().unwrap_or("nieczytelny");
            self.news.notices.push(format!(
                "Paragon {number} został wydrukowany dla zlecenia {} (zadanie {}), ale Holuto \
                 oznaczyło zadanie jako {:?}. Sprawdź wydruk i użyj „Wyjaśnij paragon”.",
                remote.order_number, remote.id, remote.state
            ));
            tracing::error!(
                job_id = %remote.id, state = ?remote.state,
                "Printed receipt conflicts with terminal Holuto state"
            );
            return Ok(Some(Blocked::OrphanedReceipt));
        }
        if remote.state.terminal() {
            self.finish(
                &mut operation,
                remote.state,
                remote.receipt_number.as_deref(),
            )?;
            return Ok(None);
        }
        if matches!(operation.device, Device::NotStarted) && remote.state == JobState::Unknown {
            self.news.notices.push(format!(
                "Zlecenie {} (zadanie {}): rejestr drukarki potwierdza, że nie wysłano żadnego \
                 polecenia fiskalnego. Rozstrzygnij zadanie jako niewydrukowane w Holuto.",
                remote.order_number, remote.id
            ));
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
                self.report(&mut operation, done(number))?;
            },
            (None, None) => {
                let result = if matches!(operation.device, Device::NotStarted) {
                    JobResult::failed("Przerwano przed wysłaniem pierwszego polecenia fiskalnego.")
                } else {
                    JobResult::unknown(INTERRUPTED)
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
        self.finish(&mut operation, state, None)?;
        Ok(None)
    }

    fn execute(&mut self, job: &Job) -> Result<(), Error> {
        let span = tracing::info_span!(
            "fiscal.receipt", sentry.op = "fiscal.receipt",
            job_id = %job.id, attempt = ?job.attempt, kind = ?job.kind
        );
        let _entered = span.enter();
        tracing::info!(job_id = %job.id, "Job claimed");
        let max_lines = self
            .status
            .as_ref()
            .map(|device| device.capabilities.max_lines);
        job.check(self.api.printer_id(), max_lines)
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
                self.news.notices.extend(notices);
                Ok(status)
            }) {
            Ok(status) => status,
            Err(error) => {
                self.report(&mut operation, JobResult::failed(&error.to_string()))?;
                return self.finish(&mut operation, JobState::Failed, None);
            },
        };
        let receipt = match self.receipt(job, &status.capabilities) {
            Ok(receipt) => receipt,
            Err(reason) => {
                self.report(&mut operation, JobResult::failed(&reason))?;
                return self.finish(&mut operation, JobState::Failed, None);
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
                self.report(&mut operation, done(printed.receipt_number))?;
                if matches!(operation.result, Some(JobResult::Done { .. })) {
                    self.finish(&mut operation, JobState::Done, None)?;
                }
                Ok(())
            },
            Err(error @ PrinterError::Uncertain(_)) => {
                tracing::error!(job_id = %job.id, "Fiscal receipt outcome is uncertain");
                self.report(&mut operation, JobResult::unknown(&error.to_string()))
            },
            Err(error) => {
                self.report(&mut operation, JobResult::failed(&error.to_string()))?;
                self.finish(&mut operation, JobState::Failed, None)
            },
        }
    }

    fn receipt(&self, job: &Job, capabilities: &Capabilities) -> Result<Receipt, String> {
        if !job.advances.is_empty() && !capabilities.advance_deductions {
            return Err("Drukarka nie obsługuje rozliczenia zaliczek.".to_owned());
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
                quantity(&item.quantity)?,
                amount("Cena", &item.unit_price)?,
                slot(item.vat_letter.slot())?,
                capabilities,
            )
            .map_err(|error| error.to_string())?;
            if line.total() != amount("Wartość pozycji", &item.total)? {
                return Err(
                    "Wartość pozycji nie zgadza się z zaokrąglonym iloczynem ilości i ceny."
                        .to_owned(),
                );
            }
            lines.push(line);
        }
        if lines.iter().map(Line::total).sum::<Grosze>() != amount("Suma zadania", &job.total)? {
            return Err("Suma pozycji nie odpowiada wartości zadania.".to_owned());
        }
        let advances = job
            .advances
            .iter()
            .map(|advance| {
                Ok(Advance {
                    vat: slot(advance.vat_letter.slot())?,
                    amount: amount("Zaliczka", &advance.amount)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let payments = job
            .payments
            .iter()
            .map(|payment| {
                Ok(Payment {
                    form: payment.form,
                    amount: amount("Płatność", &payment.amount)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let receipt = Receipt::new(
            lines,
            advances,
            payments,
            job.buyer_nip.clone(),
            &job.company_name,
            capabilities,
        )
        .map_err(|error| error.to_string())?;
        if receipt.fiscal_total() == Grosze::ZERO && !capabilities.zero_total_receipts {
            return Err(
                "Drukarka nie obsługuje paragonu opłaconego w całości zaliczkami.".to_owned(),
            );
        }
        Ok(receipt)
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
                self.news.notices.push(format!(
                    "Holuto odrzuciło dane wydrukowanego paragonu (zadanie {}). Sprawdź zegar \
                     drukarki i rozstrzygnij zadanie w Holuto po wygaśnięciu próby; wydruk nie \
                     zostanie powtórzony.",
                    operation.job_id
                ));
                tracing::error!(
                    job_id = %operation.job_id, "Holuto rejected completed fiscal evidence"
                );
                Err(error.into())
            },
            Err(error) => Err(error.into()),
        }
    }

    /// Uses the staff-supplied receipt number if the printer's was unreadable.
    fn finish(
        &mut self,
        operation: &mut ReceiptOperation,
        state: JobState,
        staff_number: Option<&str>,
    ) -> Result<(), Error> {
        self.orphaned_receipt = false;
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
                Ok(snapshot) => self.news.history = Some(snapshot),
                Err(error) => self.news.notices.push(error.to_string()),
            }
        }
        operation.settled = Some(Settlement {
            at: Timestamp::now(),
            state,
        });
        self.journal.save(&Operation::Receipt(operation.clone()))
    }

    /// Explicit local acknowledgement of evidence Holuto cannot accept.
    /// Same ready device is required; this never sends a fiscal command or result.
    pub(crate) fn acknowledge_orphaned_receipt(&mut self) -> Result<(), Error> {
        let Some(Operation::Receipt(mut operation)) = self.journal.load()? else {
            return Err(Error::Invalid("Brak paragonu do wyjaśnienia.".to_owned()));
        };
        if operation.settled.is_some() || !matches!(operation.device, Device::Completed { .. }) {
            return Err(Error::Invalid(
                "Brak wydrukowanego paragonu do wyjaśnienia.".to_owned(),
            ));
        }
        if operation.vendor != self.printer.vendor()
            || operation.device_unique_number != self.api.unique_number()
        {
            return Err(Error::Blocked(format!(
                "Podłącz drukarkę {:?} {}, która wydrukowała ten paragon.",
                operation.vendor, operation.device_unique_number
            )));
        }
        let _status = self.printer.verify(&operation.device_unique_number)?;
        let remote = self.api.job(operation.job_id)?;
        remote
            .check(self.api.printer_id(), None)
            .map_err(|problem| reconcile_error(&problem))?;
        if remote.attempt != Some(operation.attempt)
            || !matches!(remote.state, JobState::Failed | JobState::Expired)
        {
            return Err(reconcile_error(
                "stan Holuto nie pozwala na lokalne wyjaśnienie",
            ));
        }
        tracing::info!(
            job_id = %operation.job_id, state = ?remote.state,
            "Operator acknowledged orphaned receipt; no command reissued"
        );
        self.finish(&mut operation, remote.state, None)
    }
}

struct ReceiptDispatch<'a> {
    journal: &'a Journal<'a>,
    operation: &'a mut ReceiptOperation,
    waiting: &'a dyn Fn(Option<&str>),
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

    fn waiting(&mut self, problem: Option<&str>) {
        (self.waiting)(problem);
    }
}

/// Reports enter the journal only when the first command is about to be sent.
struct ReportDispatch<'a> {
    journal: &'a Journal<'a>,
    operation: &'a mut ReportOperation,
    waiting: &'a dyn Fn(Option<&str>),
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

    fn waiting(&mut self, problem: Option<&str>) {
        (self.waiting)(problem);
    }
}

/// An unreadable receipt number stays UNKNOWN until staff supply it.
fn done(receipt_number: Option<String>) -> JobResult {
    receipt_number.map_or_else(
        || JobResult::unknown(UNNUMBERED),
        |receipt_number| JobResult::Done { receipt_number },
    )
}

fn quantity(text: &str) -> Result<Quantity, String> {
    Quantity::parse(text).map_err(|error| format!("{error} ({text:?})"))
}

fn amount(field: &str, text: &str) -> Result<Grosze, String> {
    Grosze::parse(text).map_err(|error| format!("{field}: {error} ({text:?})"))
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
