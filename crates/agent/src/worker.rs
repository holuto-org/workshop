//! Sole thread for printer I/O and Holuto requests.
//!
//! Due polls run between commands; receipts and reports never interleave.
//! Shutdown finishes the current operation and publishes durable journal blocking
//! before `Stopped`, including on errors and panics. Event diagnostics omit operator
//! text and receipt contents.

use std::{
    collections::VecDeque,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use fiscal::{
    device::{Device, Vendor},
    driver::Driver,
    error::Error as PrinterError,
    link::{Connection, Link},
    report::Report,
};
use jiff::civil::Date;
use tracing::dispatcher;

use crate::{
    Error,
    agent::{Agent, Blocked, RETRY},
    api::{Api, ApiError, model::JobView, queue_form},
    device,
    documents::{BuyerNip, Document, DocumentSearch},
    journal::{Journal, Operation},
    settings::Settings,
    store::StateDir,
};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Queue {
        document: Document,
        buyer_nip: Option<BuyerNip>,
    },
    NextPage,
    PreviousPage,
    /// Restarts paging from the first page of the matching documents; `None` lists all.
    Search(Option<DocumentSearch>),
    /// Retries the server history from page one, retaining its search.
    JobsFirstPage,
    JobsNextPage,
    JobsPreviousPage,
    JobsSearch(Option<DocumentSearch>),
    DailyReport,
    MonthlyReport {
        year: i16,
        month: i8,
    },
    PeriodicReport {
        from: Date,
        to: Date,
        summary: bool,
    },
    ResolveReport,
    AcknowledgeOrphanedReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingFailure {
    Invalid,
    Communication,
    Unauthorized,
    Throttled,
    Storage,
}

pub enum Event {
    /// An operator problem, or all clear when the previous problem ended.
    Problem(Option<String>),
    /// The GUI persists detected settings and reports save failures.
    PortDetected { port: String, vendor: Vendor },
    /// The detected printer's identity; the GUI publishes it as one status.
    Connected {
        printer_id: String,
        unique_number: String,
        vendor: Vendor,
        port: String,
    },
    /// Credentials are durably saved and never included in the event.
    Paired { printer_id: String },
    PairingFailed {
        message: String,
        failure: PairingFailure,
    },
    /// `page` is 1-based.
    Documents {
        items: Vec<Document>,
        page: usize,
        has_previous: bool,
        has_next: bool,
    },
    Device {
        status: Option<Device>,
        blocked: bool,
        report_unresolved: bool,
        /// Receipt evidence the operator confirms before a local acknowledgement.
        orphaned_receipt: Option<String>,
    },
    /// Authoritative Holuto history; `page` is 1-based.
    Jobs {
        items: Vec<JobView>,
        page: usize,
        has_previous: bool,
        has_next: bool,
    },
    /// Clear stale history when Holuto cannot supply it.
    JobsUnavailable,
    /// Holuto could not list documents; an empty table must not read as nothing to print.
    DocumentsUnavailable,
    /// Releases the GUI submission guard even when a command is refused.
    CommandCompleted,
    /// Safe to join the worker.
    Stopped,
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Problem(Some(_)) => f.write_str("Problem(..)"),
            Self::Problem(None) => f.write_str("Problem(None)"),
            Self::PortDetected { .. } => f.write_str("PortDetected(..)"),
            Self::Paired { printer_id } => f
                .debug_struct("Paired")
                .field("printer_id", printer_id)
                .finish(),
            Self::PairingFailed { failure, .. } => f
                .debug_struct("PairingFailed")
                .field("failure", failure)
                .finish_non_exhaustive(),
            Self::Connected {
                printer_id,
                unique_number,
                ..
            } => f
                .debug_struct("Connected")
                .field("printer_id", printer_id)
                .field("unique_number", unique_number)
                .finish_non_exhaustive(),
            Self::Documents {
                items,
                page,
                has_previous,
                has_next,
            } => f
                .debug_struct("Documents")
                .field("count", &items.len())
                .field("page", page)
                .field("has_previous", has_previous)
                .field("has_next", has_next)
                .finish(),
            Self::Device {
                status,
                blocked,
                report_unresolved,
                orphaned_receipt,
            } => f
                .debug_struct("Device")
                .field("ready", &status.as_ref().map(Device::ready))
                .field("blocked", blocked)
                .field("report_unresolved", report_unresolved)
                .field("orphaned_receipt", &orphaned_receipt.is_some())
                .finish(),
            Self::Jobs {
                items,
                page,
                has_previous,
                has_next,
            } => f
                .debug_struct("Jobs")
                .field("count", &items.len())
                .field("page", page)
                .field("has_previous", has_previous)
                .field("has_next", has_next)
                .finish(),
            Self::JobsUnavailable => f.write_str("JobsUnavailable"),
            Self::DocumentsUnavailable => f.write_str("DocumentsUnavailable"),
            Self::CommandCompleted => f.write_str("CommandCompleted"),
            Self::Stopped => f.write_str("Stopped"),
        }
    }
}

enum Message {
    Command(Command),
    Stop,
}

#[derive(Debug)]
pub struct Worker {
    messages: Sender<Message>,
    thread: JoinHandle<()>,
}

/// Initial list criteria; supplied before the worker can publish either feed.
#[derive(Debug, Default)]
pub struct Searches {
    pub documents: Option<DocumentSearch>,
    pub jobs: Option<DocumentSearch>,
}

impl Worker {
    /// Never opens a printer; reports success only after saving credentials.
    pub fn pair<S>(
        dir: Arc<StateDir>,
        mut settings: Settings,
        code: String,
        sink: S,
    ) -> Result<Self, Error>
    where
        S: Fn(Event) + Send + Sync + 'static,
    {
        Self::start(dir, sink, true, move |dir, _inbox, sink| {
            settings.validate_configuration()?;
            let paired = Api::pair(&settings.api_url, &code)?;
            settings.token = paired.token;
            settings.save(dir)?;
            sink(Event::Paired {
                printer_id: paired.printer_id.to_string(),
            });
            Ok(())
        })
    }

    pub fn spawn<S>(
        dir: Arc<StateDir>,
        settings: Settings,
        link: Arc<dyn Link>,
        searches: Searches,
        sink: S,
    ) -> Result<Self, Error>
    where
        S: Fn(Event) + Send + Sync + 'static,
    {
        Self::start(dir, sink, false, move |dir, inbox, sink| {
            run(dir, &settings, &link, searches, inbox, sink)
        })
    }

    fn start<S, F>(dir: Arc<StateDir>, sink: S, pairing: bool, operation: F) -> Result<Self, Error>
    where
        S: Fn(Event) + Send + Sync + 'static,
        F: FnOnce(&StateDir, &Receiver<Message>, &S) -> Result<(), Error> + Send + 'static,
    {
        let (messages, inbox) = mpsc::channel();
        let path = dir.path().to_owned();
        let dispatch = dispatcher::get_default(Clone::clone);
        #[cfg(test)]
        let clock = tests::clock();
        let thread = thread::Builder::new()
            .name("printer worker".to_owned())
            .spawn(move || {
                #[cfg(test)]
                let _clock = tests::ClockScope::enter(clock);
                dispatcher::with_default(&dispatch, || {
                    match catch_unwind(AssertUnwindSafe(|| operation(&dir, &inbox, &sink))) {
                        Ok(Ok(())) => {},
                        Ok(Err(error)) => {
                            trace_failure(&error);
                            if pairing {
                                sink(pairing_failed(&error));
                            } else {
                                sink(Event::Problem(Some(format!("Błąd: {error}"))));
                            }
                        },
                        Err(_payload) => {
                            tracing::error!("Worker panicked");
                            let text = "Wątek drukarki został przerwany. Sprawdź wydruk przed \
                                   ponownym uruchomieniem."
                                .to_owned();
                            if pairing {
                                sink(Event::PairingFailed {
                                    message: text,
                                    failure: PairingFailure::Communication,
                                });
                            } else {
                                sink(Event::Problem(Some(text)));
                            }
                        },
                    }
                    if !pairing {
                        let block = journal_block(&dir);
                        sink(Event::Device {
                            status: None,
                            blocked: block.blocked,
                            report_unresolved: block.report_unresolved,
                            orphaned_receipt: None,
                        });
                    }
                    sink(Event::Stopped);
                });
            })
            .map_err(|source| Error::Store {
                action: "Nie można uruchomić wątku drukarki",
                path,
                source,
            })?;
        Ok(Self { messages, thread })
    }

    pub fn submit(&self, command: Command) {
        // A closed inbox means the worker has stopped.
        let _sent = self.messages.send(Message::Command(command));
    }

    /// Stops after the current operation; emits `Event::Stopped`.
    pub fn stop(&self) {
        let _sent = self.messages.send(Message::Stop);
    }

    /// Call only after `Event::Stopped` to avoid blocking the GUI.
    pub fn join(self) {
        let _finished = self.thread.join();
    }
}

/// `Event::Device` block flags known from the journal alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalBlock {
    pub blocked: bool,
    pub report_unresolved: bool,
}

/// For display before a worker has read the journal; never writes it or touches the printer
/// or Holuto. An unreadable journal blocks; an orphaned receipt needs Holuto, so it shows
/// only as `blocked`.
pub fn journal_block(dir: &StateDir) -> JournalBlock {
    let blocking = Journal::new(dir)
        .load()
        .map(|operation| operation.filter(Operation::blocks));
    JournalBlock {
        blocked: !matches!(blocking, Ok(None)),
        report_unresolved: matches!(blocking, Ok(Some(Operation::Report(_)))),
    }
}

fn pairing_failed(error: &Error) -> Event {
    let failure = match error {
        Error::Api(ApiError::Unauthorized) => PairingFailure::Unauthorized,
        Error::Api(ApiError::Status { status: 429, .. }) => PairingFailure::Throttled,
        Error::Invalid(_) | Error::Api(ApiError::Status { status: 400, .. }) => {
            PairingFailure::Invalid
        },
        Error::Store { .. } | Error::Busy(_) => PairingFailure::Storage,
        Error::Api(ApiError::Status { .. } | ApiError::Transport(_) | ApiError::Response(_))
        | Error::Printer(_)
        | Error::Blocked(_) => PairingFailure::Communication,
    };
    Event::PairingFailed {
        message: match failure {
            PairingFailure::Invalid => match error {
                Error::Invalid(message) => message.clone(),
                Error::Api(_)
                | Error::Store { .. }
                | Error::Printer(_)
                | Error::Blocked(_)
                | Error::Busy(_) => "Nieprawidłowy, wykorzystany lub wygasły kod parowania. \
                     Wygeneruj nowy kod w Holuto."
                    .to_owned(),
            },
            PairingFailure::Throttled => "Zbyt wiele prób parowania. Spróbuj później.".to_owned(),
            PairingFailure::Communication
            | PairingFailure::Unauthorized
            | PairingFailure::Storage => error.to_string(),
        },
        failure,
    }
}

fn run(
    dir: &StateDir,
    settings: &Settings,
    link: &Arc<dyn Link>,
    searches: Searches,
    inbox: &Receiver<Message>,
    sink: &(dyn Fn(Event) + Sync),
) -> Result<(), Error> {
    let mut queue = VecDeque::new();
    let Some((printer, status)) = detect_printer(dir, settings, link, inbox, sink, &mut queue)?
    else {
        return Ok(());
    };
    let port = printer.connection().port.clone();
    if port != settings.port || Some(status.vendor) != settings.vendor {
        sink(Event::PortDetected {
            port: port.clone(),
            vendor: status.vendor,
        });
    }
    let api = Api::new(
        &settings.api_url,
        settings.token.as_str(),
        &status.unique_number,
    )?;
    let waiting = |problem: Option<&str>| {
        if problem.is_some() {
            tracing::warn!("Printer requires operator intervention");
        }
        sink(Event::Problem(problem.map(|problem| {
            format!("Drukarka czeka: {problem}. Usuń problem, a drukowanie będzie kontynuowane.")
        })));
    };
    let mut agent = Agent::new(printer, api, dir, &waiting);
    let notices = agent.describe(status.clone())?;
    if !start_session(&mut agent, inbox, sink, &mut queue)? {
        return Ok(());
    }
    let printer_id = agent
        .api()
        .printer_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    tracing::info!(
        printer_id,
        device_unique_number = status.unique_number,
        "Connected"
    );
    sink(Event::Connected {
        printer_id,
        unique_number: status.unique_number.clone(),
        vendor: status.vendor,
        port,
    });
    sink(Event::Problem(None));
    for notice in notices {
        sink(Event::Problem(Some(notice)));
    }
    Session {
        agent,
        pager: Pager::new(searches.documents),
        jobs: Pager::new(searches.jobs),
        queue,
        sink,
        error_shown: false,
    }
    .run(inbox)
}

fn start_session(
    agent: &mut Agent<'_>,
    inbox: &Receiver<Message>,
    sink: &dyn Fn(Event),
    queue: &mut VecDeque<Command>,
) -> Result<bool, Error> {
    loop {
        let session = agent.session();
        let due = now() + RETRY;
        let refresh = match session {
            Ok(true) => return Ok(true),
            Ok(false) => {
                publish_device(agent, sink);
                sink(Event::Problem(Some(agent.not_ready_notice())));
                true
            },
            Err(error) if retry_start(&error) => {
                tracing::warn!("Holuto session unavailable; retrying in 15 seconds");
                sink(Event::Problem(Some(format!(
                    "Nie można połączyć z Holuto: {error}. Ponowna próba za 15 sekund."
                ))));
                sink(Event::JobsUnavailable);
                false
            },
            Err(error) => return Err(error),
        };
        if !wait(due, inbox, queue) {
            return Ok(false);
        }
        if refresh {
            match agent.refresh() {
                Ok(notices) => {
                    for text in notices {
                        sink(Event::Problem(Some(text)));
                    }
                },
                Err(error) => sink(Event::Problem(Some(format!("Błąd: {error}")))),
            }
        }
    }
}

type Detected = (Box<dyn Driver>, Device);

/// A printer switched on after an unattended login start must not end processing.
/// Detection sends only read-only identity probes, so repeating it is safe; `None` means stop.
fn detect_printer(
    dir: &StateDir,
    settings: &Settings,
    link: &Arc<dyn Link>,
    inbox: &Receiver<Message>,
    sink: &dyn Fn(Event),
    queue: &mut VecDeque<Command>,
) -> Result<Option<Detected>, Error> {
    // Unresolved operations forbid switching devices or scanning other ports.
    let blocking = Journal::new(dir).load()?.filter(Operation::blocks);
    let remembered = blocking.as_ref().map_or(settings.vendor, |operation| {
        Some(match operation {
            Operation::Receipt(receipt) => receipt.vendor,
            Operation::Report(report) => report.vendor,
        })
    });
    loop {
        let connection = Connection {
            port: settings.port.clone(),
            baud_rate: settings.baud_rate,
        };
        let error =
            match device::detect(Arc::clone(link), connection, remembered, blocking.is_none()) {
                Ok(found) => return Ok(Some(found)),
                Err(error) => error,
            };
        // Counted from the failure, before announcing it: scanning silent ports can outlast
        // the interval.
        let due = now() + RETRY;
        tracing::warn!("Printer unavailable at start; retrying in 15 seconds");
        sink(Event::JobsUnavailable);
        sink(Event::Problem(Some(format!(
            "Drukarka niedostępna: {error}. Ponowna próba za 15 sekund."
        ))));
        if !wait(due, inbox, queue) {
            return Ok(None);
        }
    }
}

/// Queues commands until the retry is due; `false` means stop.
fn wait(due: Instant, inbox: &Receiver<Message>, queue: &mut VecDeque<Command>) -> bool {
    loop {
        match inbox.recv_timeout(until(due)) {
            Ok(Message::Command(command)) => queue.push_back(command),
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
            Err(RecvTimeoutError::Timeout) => return true,
        }
    }
}

fn publish_device(agent: &Agent<'_>, sink: &dyn Fn(Event)) {
    let blocked = agent.blocked();
    let report_unresolved = matches!(blocked, Ok(Some(Blocked::Report)));
    let orphaned_receipt = matches!(blocked, Ok(Some(Blocked::OrphanedReceipt)))
        .then(|| agent.orphan_evidence().map(str::to_owned))
        .flatten();
    sink(Event::Device {
        status: agent.status().cloned(),
        // An unreadable journal also blocks printing.
        blocked: !matches!(blocked, Ok(None)),
        report_unresolved,
        orphaned_receipt,
    });
}

/// Transient Holuto failures must not end processing started unattended at login.
///
/// A 403 is an inactive branch or suspended company: the pairing stays valid, so the agent waits
/// for it to become available again, as it already does after its session started.
const fn retry_start(error: &Error) -> bool {
    matches!(
        error,
        Error::Api(
            ApiError::Transport(_)
                | ApiError::Status {
                    status: 403 | 408 | 429 | 500..=599,
                    ..
                }
        )
    )
}

fn trace_failure(error: &Error) {
    match error {
        Error::Api(ApiError::Response(_))
        | Error::Blocked(_)
        | Error::Store { .. }
        | Error::Printer(PrinterError::Uncertain(_)) => {
            tracing::error!("Worker operation requires attention");
        },
        Error::Api(ApiError::Unauthorized | ApiError::Status { .. } | ApiError::Transport(_))
        | Error::Printer(_)
        | Error::Invalid(_)
        | Error::Busy(_) => tracing::warn!("Worker operation failed"),
    }
}

/// The worker clock; tests freeze it so background polls cannot interleave assertions.
fn now() -> Instant {
    #[cfg(test)]
    {
        tests::now()
    }
    #[cfg(not(test))]
    {
        Instant::now()
    }
}

fn until(due: Instant) -> Duration {
    due.saturating_duration_since(now())
}

struct Session<'a> {
    agent: Agent<'a>,
    pager: Pager,
    jobs: Pager,
    queue: VecDeque<Command>,
    sink: &'a dyn Fn(Event),
    /// The footer shows an error, which the next successful operation clears.
    /// Fiscal notices are not errors: only a newer problem replaces them.
    error_shown: bool,
}

impl Session<'_> {
    fn run(mut self, inbox: &Receiver<Message>) -> Result<(), Error> {
        // Inbox activity must not postpone background polling.
        let mut due = now();
        loop {
            if self.drain(inbox) {
                return Ok(());
            }
            while let Some(command) = self.queue.pop_front() {
                let navigation = matches!(
                    command,
                    Command::NextPage
                        | Command::PreviousPage
                        | Command::Search(_)
                        | Command::JobsFirstPage
                        | Command::JobsNextPage
                        | Command::JobsPreviousPage
                        | Command::JobsSearch(_)
                );
                let done = self.command(command);
                let _changed = self.publish_news();
                if let Err(error) = done {
                    self.report_error(&error);
                    if matches!(error, Error::Api(ApiError::Unauthorized)) {
                        if !navigation {
                            (self.sink)(Event::CommandCompleted);
                        }
                        return Ok(());
                    }
                }
                if !navigation {
                    (self.sink)(Event::CommandCompleted);
                    due = now();
                }
                if self.drain(inbox) {
                    return Ok(());
                }
                if until(due).is_zero() {
                    break;
                }
            }
            if until(due).is_zero() {
                let delay = match self.poll(inbox) {
                    Ok(Some(delay)) => {
                        self.recovered();
                        delay
                    },
                    Ok(None) => return Ok(()),
                    Err(error @ Error::Api(ApiError::Unauthorized)) => return Err(error),
                    Err(error) => {
                        self.report_error(&error);
                        RETRY
                    },
                };
                due = now() + delay;
            }
            if !self.queue.is_empty() {
                continue;
            }
            match inbox.recv_timeout(until(due)) {
                Ok(Message::Command(command)) => self.queue.push_back(command),
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {},
            }
        }
    }

    fn drain(&mut self, inbox: &Receiver<Message>) -> bool {
        loop {
            match inbox.try_recv() {
                Ok(Message::Command(command)) => self.queue.push_back(command),
                Ok(Message::Stop) | Err(TryRecvError::Disconnected) => return true,
                Err(TryRecvError::Empty) => return false,
            }
        }
    }

    fn poll(&mut self, inbox: &Receiver<Message>) -> Result<Option<Duration>, Error> {
        if self.drain(inbox) {
            return Ok(None);
        }
        // Show web-queued work before take can consume it. Listing never heartbeats or claims.
        let _changed = self.publish_news();
        let mut jobs = self.refresh_jobs();
        if matches!(jobs, Err(ApiError::Unauthorized)) {
            return Err(ApiError::Unauthorized.into());
        }
        let delay = self.agent.step();
        let jobs_changed = self.publish_news();
        if matches!(delay, Err(Error::Api(ApiError::Unauthorized))) {
            return Err(ApiError::Unauthorized.into());
        }
        if self.drain(inbox) {
            return Ok(None);
        }
        // Printer and settlement failures must not hide server lists.
        let refreshed = self.refresh_documents();
        if matches!(refreshed, Err(ApiError::Unauthorized)) {
            return Err(ApiError::Unauthorized.into());
        }
        if jobs_changed {
            jobs = self.refresh_jobs();
        }
        if matches!(jobs, Err(ApiError::Unauthorized)) {
            return Err(ApiError::Unauthorized.into());
        }
        let delay = delay?;
        refreshed?;
        jobs?;
        self.publish_device();
        Ok(Some(delay))
    }

    fn command(&mut self, command: Command) -> Result<(), Error> {
        let report = match command {
            Command::Queue {
                document,
                buyer_nip,
            } => {
                let form = queue_form(&document, buyer_nip)?;
                self.agent.api().queue(&form)?;
                self.jobs.first();
                self.clear_problem();
                return Ok(());
            },
            Command::NextPage => {
                self.pager.next();
                self.refresh_documents()?;
                return Ok(());
            },
            Command::PreviousPage => {
                self.pager.previous();
                self.refresh_documents()?;
                return Ok(());
            },
            Command::Search(search) => {
                // Polls keep the new search even if this first fetch fails.
                self.pager = Pager::new(search);
                self.refresh_documents()?;
                return Ok(());
            },
            Command::JobsFirstPage => {
                self.jobs.first();
                self.refresh_jobs()?;
                return Ok(());
            },
            Command::JobsNextPage => {
                self.jobs.next();
                self.refresh_jobs()?;
                return Ok(());
            },
            Command::JobsPreviousPage => {
                self.jobs.previous();
                self.refresh_jobs()?;
                return Ok(());
            },
            Command::JobsSearch(search) => {
                self.jobs = Pager::new(search);
                self.refresh_jobs()?;
                return Ok(());
            },
            Command::ResolveReport => {
                self.agent.resolve_report()?;
                self.publish_device();
                self.clear_problem();
                return Ok(());
            },
            Command::AcknowledgeOrphanedReceipt => {
                self.agent.acknowledge_orphaned_receipt()?;
                self.publish_device();
                self.clear_problem();
                return Ok(());
            },
            Command::DailyReport => Report::Daily,
            Command::MonthlyReport { year, month } => Report::Monthly { year, month },
            Command::PeriodicReport { from, to, summary } => Report::Periodic { from, to, summary },
        };
        let printed = self.agent.run_report(report);
        self.publish_device();
        printed?;
        self.clear_problem();
        Ok(())
    }

    fn publish_device(&self) {
        publish_device(&self.agent, self.sink);
    }

    /// Failures can still produce notices or change the server list.
    fn publish_news(&mut self) -> bool {
        let news = self.agent.take_news();
        for notice in news.notices {
            self.problem(notice);
        }
        if news.jobs_changed {
            self.jobs.first();
        }
        news.jobs_changed
    }

    fn refresh_documents(&mut self) -> Result<(), ApiError> {
        match self.pager.refresh(self.agent.api()) {
            Ok(event) => {
                (self.sink)(event);
                Ok(())
            },
            Err(error) => {
                (self.sink)(Event::DocumentsUnavailable);
                Err(error)
            },
        }
    }

    fn refresh_jobs(&mut self) -> Result<(), ApiError> {
        match self.jobs.refresh_jobs(self.agent.api()) {
            Ok(event) => {
                (self.sink)(event);
                Ok(())
            },
            Err(error) => {
                (self.sink)(Event::JobsUnavailable);
                Err(error)
            },
        }
    }

    fn report_error(&mut self, error: &Error) {
        trace_failure(error);
        self.problem(format!("Błąd: {error}"));
        self.error_shown = true;
        self.publish_device();
    }

    fn recovered(&mut self) {
        if self.error_shown {
            self.clear_problem();
        }
    }

    fn problem(&mut self, text: String) {
        self.error_shown = false;
        (self.sink)(Event::Problem(Some(text)));
    }

    fn clear_problem(&mut self) {
        self.error_shown = false;
        (self.sink)(Event::Problem(None));
    }
}

/// Both feeds follow server cursors. Failed navigation retains every pending click.
struct Pager {
    cursor: Option<String>,
    page: usize,
    next: Option<String>,
    previous: Option<String>,
    requested: isize,
    search: Option<DocumentSearch>,
}

impl Pager {
    const fn new(search: Option<DocumentSearch>) -> Self {
        Self {
            cursor: None,
            page: 1,
            next: None,
            previous: None,
            requested: 0,
            search,
        }
    }

    fn first(&mut self) {
        self.cursor = None;
        self.page = 1;
        self.previous = None;
        self.next = None;
        self.requested = 0;
    }

    const fn next(&mut self) {
        if self.requested != 0 || self.next.is_some() {
            self.requested = self.requested.saturating_add(1);
        }
    }

    const fn previous(&mut self) {
        if self.requested != 0 || self.previous.is_some() {
            self.requested = self.requested.saturating_sub(1);
        }
    }

    fn refresh(&mut self, api: &Api) -> Result<Event, ApiError> {
        let (items, page, has_previous, has_next) = self.refresh_with(|cursor, search| {
            api.documents(cursor, search).map(|page| CursorPage {
                items: page.items,
                next: page.next_cursor,
                previous: page.previous_cursor,
            })
        })?;
        Ok(Event::Documents {
            items,
            page,
            has_previous,
            has_next,
        })
    }

    fn refresh_jobs(&mut self, api: &Api) -> Result<Event, ApiError> {
        let (items, page, has_previous, has_next) = self.refresh_with(|cursor, search| {
            api.jobs(cursor, search).map(|page| CursorPage {
                items: page.items,
                next: page.next_cursor,
                previous: page.previous_cursor,
            })
        })?;
        Ok(Event::Jobs {
            items,
            page,
            has_previous,
            has_next,
        })
    }

    fn refresh_with<T, F>(&mut self, mut fetch: F) -> Result<(Vec<T>, usize, bool, bool), ApiError>
    where
        F: FnMut(Option<&str>, Option<&DocumentSearch>) -> Result<CursorPage<T>, ApiError>,
    {
        let mut cursor = self.cursor.clone();
        let mut number = self.page;
        let forward = self.requested > 0;
        let advance = |number: usize| {
            if forward {
                number.saturating_add(1)
            } else {
                number.saturating_sub(1).max(1)
            }
        };
        let mut remaining = self.requested.unsigned_abs();
        let opening = if forward { &self.next } else { &self.previous };
        if remaining > 0 && opening.is_some() {
            cursor.clone_from(opening);
            number = advance(number);
            remaining -= 1;
        } else {
            remaining = 0;
        }
        let mut page = fetch(cursor.as_deref(), self.search.as_ref())?;
        while remaining > 0 {
            let continuation = if forward { &page.next } else { &page.previous };
            let Some(next) = continuation else {
                break;
            };
            cursor = Some(next.clone());
            page = fetch(cursor.as_deref(), self.search.as_ref())?;
            number = advance(number);
            remaining -= 1;
        }
        self.cursor = cursor;
        self.page = number;
        self.previous = page.previous;
        self.next = page.next;
        self.requested = 0;
        Ok((
            page.items,
            number,
            self.previous.is_some(),
            self.next.is_some(),
        ))
    }
}

struct CursorPage<T> {
    items: Vec<T>,
    next: Option<String>,
    previous: Option<String>,
}

#[cfg(test)]
mod tests;
