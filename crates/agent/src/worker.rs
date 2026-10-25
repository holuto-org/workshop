//! Sole thread for printer I/O and Holuto requests.
//!
//! Commands yield to due polls between operations; receipts and reports never interleave.
//! Cooperative shutdown finishes the current operation before stopping.
//! Event diagnostics expose metadata, never operator text or receipt contents.

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
    error::Error as PrinterError,
    link::{Connection, Link},
    report::Report,
};
use jiff::civil::Date;
use tracing::dispatcher;

use crate::{
    Error,
    agent::{Agent, Blocked, RETRY},
    api::{Api, ApiError, model::Page, queue_form},
    device,
    documents::{BuyerNip, Document},
    history::{self, HistoryEntry},
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
pub enum Tone {
    Info,
    Success,
    Error,
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
    Status {
        text: String,
        tone: Tone,
    },
    /// The GUI owns settings persistence and reports save failures.
    PortDetected {
        port: String,
        vendor: Vendor,
    },
    Connected {
        printer_id: String,
        serial: String,
        port: String,
    },
    /// Credentials have already been durably saved; this event contains no secret.
    Paired {
        printer_id: String,
    },
    PairingFailed {
        message: String,
        failure: PairingFailure,
    },
    /// `page` is 1-based.
    Documents {
        items: Vec<Document>,
        page: usize,
        has_next: bool,
    },
    Device {
        status: Option<Device>,
        blocked: bool,
        report_unresolved: bool,
        orphaned_receipt: bool,
    },
    /// Bounded newest-first snapshot; rendering never reads worker-owned storage.
    HistoryChanged(Vec<HistoryEntry>),
    /// Releases a GUI submission guard, including after a refused command.
    CommandCompleted,
    /// Safe to join the worker.
    Stopped,
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status { tone, .. } => f
                .debug_struct("Status")
                .field("tone", tone)
                .finish_non_exhaustive(),
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
                printer_id, serial, ..
            } => f
                .debug_struct("Connected")
                .field("printer_id", printer_id)
                .field("serial", serial)
                .finish_non_exhaustive(),
            Self::Documents {
                items,
                page,
                has_next,
            } => f
                .debug_struct("Documents")
                .field("count", &items.len())
                .field("page", page)
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
                .field("orphaned_receipt", orphaned_receipt)
                .finish(),
            Self::HistoryChanged(entries) => f
                .debug_struct("HistoryChanged")
                .field("count", &entries.len())
                .finish(),
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

impl Worker {
    /// Pairs without constructing or opening a printer; success follows durable storage.
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
        sink: S,
    ) -> Result<Self, Error>
    where
        S: Fn(Event) + Send + Sync + 'static,
    {
        Self::start(dir, sink, false, move |dir, inbox, sink| {
            run(dir, &settings, link, inbox, sink)
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
                                sink(Event::Status {
                                    text: format!("Błąd: {error}"),
                                    tone: Tone::Error,
                                });
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
                                sink(Event::Status {
                                    text,
                                    tone: Tone::Error,
                                });
                            }
                        },
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
        // Failed sends mean the worker finished and reported `Event::Stopped`.
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

fn pairing_failed(error: &Error) -> Event {
    let failure = match error {
        Error::Api(ApiError::Unauthorized) => PairingFailure::Unauthorized,
        Error::Api(ApiError::Status { status: 429, .. }) => PairingFailure::Throttled,
        Error::Invalid(_) | Error::Api(ApiError::Status { status: 400, .. }) => {
            PairingFailure::Invalid
        },
        Error::Store { .. } => PairingFailure::Storage,
        Error::Api(ApiError::Status { .. } | ApiError::Transport(_) | ApiError::Response(_))
        | Error::Printer(_)
        | Error::Blocked(_) => PairingFailure::Communication,
    };
    Event::PairingFailed {
        message: match failure {
            PairingFailure::Invalid => match error {
                Error::Invalid(message) => message.clone(),
                Error::Api(_) | Error::Store { .. } | Error::Printer(_) | Error::Blocked(_) => {
                    "Nieprawidłowy, wykorzystany lub wygasły kod parowania. \
                     Wygeneruj nowy kod w Holuto."
                        .to_owned()
                },
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
    link: Arc<dyn Link>,
    inbox: &Receiver<Message>,
    sink: &(dyn Fn(Event) + Sync),
) -> Result<(), Error> {
    sink(Event::Status {
        text: "Łączenie z drukarką i Holuto…".to_owned(),
        tone: Tone::Info,
    });
    match history::load(dir) {
        Ok(entries) => sink(Event::HistoryChanged(entries)),
        Err(error) => {
            tracing::warn!("Could not load the receipt display history");
            sink(Event::Status {
                text: error.to_string(),
                tone: Tone::Error,
            });
        },
    }
    // Unresolved operations forbid switching devices or scanning other ports.
    let blocking = Journal::new(dir).load()?.filter(Operation::blocks);
    let remembered = blocking.as_ref().map_or(settings.vendor, |operation| {
        Some(match operation {
            Operation::Receipt(receipt) => receipt.vendor,
            Operation::Report(report) => report.vendor,
        })
    });
    let (printer, status) = device::detect(
        link,
        Connection {
            port: settings.port.clone(),
            baud_rate: settings.baud_rate,
        },
        remembered,
        blocking.is_none(),
    )?;
    let port = printer.connection().port.clone();
    if port != settings.port || Some(status.vendor) != settings.vendor {
        sink(Event::PortDetected {
            port: port.clone(),
            vendor: status.vendor,
        });
    }
    let api = Api::new(&settings.api_url, settings.token.as_str(), &status.serial)?;
    let waiting = |problem: Option<&str>| {
        if problem.is_some() {
            tracing::warn!("Printer requires operator intervention");
        }
        sink(problem.map_or_else(
            || Event::Status {
                text: "Drukowanie wznowione.".to_owned(),
                tone: Tone::Info,
            },
            |problem| Event::Status {
                text: format!(
                    "Drukarka czeka: {problem}. Usuń problem, a drukowanie będzie kontynuowane."
                ),
                tone: Tone::Error,
            },
        ));
    };
    let mut agent = Agent::new(printer, api, dir, &waiting);
    let notices = agent.describe(status.clone())?;
    let mut queue = VecDeque::new();
    if !start_session(&mut agent, inbox, sink, &mut queue)? {
        return Ok(());
    }
    let printer_id = agent
        .api()
        .printer_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    tracing::info!(printer_id, device_serial = status.serial, "Connected");
    sink(Event::Connected {
        printer_id,
        serial: status.serial.clone(),
        port,
    });
    sink(Event::Status {
        text: format!("Połączono: {}. Oczekiwanie na zlecenia.", status.serial),
        tone: Tone::Success,
    });
    for notice in notices {
        sink(Event::Status {
            text: notice,
            tone: Tone::Error,
        });
    }
    Session {
        agent,
        pager: Pager::default(),
        queue,
        sink,
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
        let cadence = Cadence {
            due: Cadence::now() + RETRY,
        };
        let refresh = match session {
            Ok(true) => return Ok(true),
            Ok(false) => {
                publish_device(agent, sink);
                sink(Event::Status {
                    text: agent.not_ready_notice(),
                    tone: Tone::Error,
                });
                true
            },
            Err(error) if retry_start(&error) => {
                tracing::warn!("Holuto session unavailable; retrying in 15 seconds");
                sink(Event::Status {
                    text: format!(
                        "Nie można połączyć z Holuto: {error}. Ponowna próba za 15 sekund."
                    ),
                    tone: Tone::Error,
                });
                false
            },
            Err(error) => return Err(error),
        };
        loop {
            match inbox.recv_timeout(cadence.remaining(Cadence::now())) {
                Ok(Message::Command(command)) => queue.push_back(command),
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return Ok(false),
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        if refresh {
            match agent.refresh() {
                Ok(notices) => {
                    for text in notices {
                        sink(Event::Status {
                            text,
                            tone: Tone::Error,
                        });
                    }
                },
                Err(error) => sink(Event::Status {
                    text: format!("Błąd: {error}"),
                    tone: Tone::Error,
                }),
            }
        }
    }
}

fn publish_device(agent: &Agent<'_>, sink: &dyn Fn(Event)) {
    let blocked = agent.blocked();
    let report_unresolved = matches!(blocked, Ok(Some(Blocked::Report)));
    let orphaned_receipt = matches!(blocked, Ok(Some(Blocked::OrphanedReceipt)));
    sink(Event::Device {
        status: agent.status().cloned(),
        // An unreadable journal also blocks printing.
        blocked: !matches!(blocked, Ok(None)),
        report_unresolved,
        orphaned_receipt,
    });
}

const fn retry_start(error: &Error) -> bool {
    matches!(
        error,
        Error::Api(
            ApiError::Transport(_)
                | ApiError::Status {
                    status: 500..=599,
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
        | Error::Invalid(_) => tracing::warn!("Worker operation failed"),
    }
}

/// Message wakes never move the background deadline.
struct Cadence {
    due: Instant,
}

impl Cadence {
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

    fn schedule(&mut self, now: Instant, delay: Duration) {
        self.due = now + delay;
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.due.saturating_duration_since(now)
    }
}

struct Session<'a> {
    agent: Agent<'a>,
    pager: Pager,
    queue: VecDeque<Command>,
    sink: &'a dyn Fn(Event),
}

impl Session<'_> {
    fn run(mut self, inbox: &Receiver<Message>) -> Result<(), Error> {
        let mut cadence = Cadence {
            due: Cadence::now(),
        };
        loop {
            if self.drain(inbox) {
                return Ok(());
            }
            while let Some(command) = self.queue.pop_front() {
                let navigation = matches!(command, Command::NextPage | Command::PreviousPage);
                let done = self.command(command);
                self.publish_news();
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
                    cadence.schedule(Cadence::now(), Duration::ZERO);
                }
                if self.drain(inbox) {
                    return Ok(());
                }
                if cadence.remaining(Cadence::now()).is_zero() {
                    break;
                }
            }
            if cadence.remaining(Cadence::now()).is_zero() {
                let delay = match self.poll(inbox) {
                    Ok(Some(delay)) => delay,
                    Ok(None) => return Ok(()),
                    Err(error @ Error::Api(ApiError::Unauthorized)) => return Err(error),
                    Err(error) => {
                        self.report_error(&error);
                        RETRY
                    },
                };
                cadence.schedule(Cadence::now(), delay);
            }
            if !self.queue.is_empty() {
                continue;
            }
            match inbox.recv_timeout(cadence.remaining(Cadence::now())) {
                Ok(Message::Command(command)) => self.queue.push_back(command),
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {},
            }
        }
    }

    /// Drains commands; returns whether stop was requested or the owner is gone.
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
        let delay = self.agent.step();
        self.publish_news();
        if matches!(delay, Err(Error::Api(ApiError::Unauthorized))) {
            return Err(ApiError::Unauthorized.into());
        }
        if self.drain(inbox) {
            return Ok(None);
        }
        // Printer and settlement errors must not prevent fetching Holuto documents.
        let refreshed = self
            .pager
            .refresh(self.agent.api())
            .map(|page| (self.sink)(page));
        if matches!(refreshed, Err(ApiError::Unauthorized)) {
            return Err(ApiError::Unauthorized.into());
        }
        let delay = delay?;
        refreshed?;
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
                self.say(
                    "Zlecenie dodano do kolejki Holuto.".to_owned(),
                    Tone::Success,
                );
                return Ok(());
            },
            Command::NextPage => {
                self.pager.next();
                (self.sink)(self.pager.refresh(self.agent.api())?);
                return Ok(());
            },
            Command::PreviousPage => {
                self.pager.previous();
                (self.sink)(self.pager.refresh(self.agent.api())?);
                return Ok(());
            },
            Command::ResolveReport => {
                self.agent.resolve_report()?;
                self.publish_device();
                self.say("Wyjaśniono raport.".to_owned(), Tone::Success);
                return Ok(());
            },
            Command::AcknowledgeOrphanedReceipt => {
                self.agent.acknowledge_orphaned_receipt()?;
                self.publish_device();
                self.say("Wyjaśniono osierocony paragon.".to_owned(), Tone::Success);
                return Ok(());
            },
            Command::DailyReport => Report::Daily,
            Command::MonthlyReport { year, month } => Report::Monthly { year, month },
            Command::PeriodicReport { from, to, summary } => Report::Periodic { from, to, summary },
        };
        self.say(format!("{}: drukowanie…", report.label()), Tone::Info);
        let printed = self.agent.run_report(report);
        self.publish_device();
        printed?;
        self.say(format!("{} wydrukowany.", report.label()), Tone::Success);
        Ok(())
    }

    fn publish_device(&self) {
        publish_device(&self.agent, self.sink);
    }

    /// Publish even if the call that produced the news failed.
    fn publish_news(&mut self) {
        let news = self.agent.take_news();
        for notice in news.notices {
            self.say(notice, Tone::Error);
        }
        if let Some(entries) = news.history {
            (self.sink)(Event::HistoryChanged(entries));
        }
    }

    fn report_error(&self, error: &Error) {
        trace_failure(error);
        self.say(format!("Błąd: {error}"), Tone::Error);
        self.publish_device();
    }

    fn say(&self, text: String, tone: Tone) {
        (self.sink)(Event::Status { text, tone });
    }
}

/// Refreshes the shown page; navigation follows returned cursors up to the pending target.
///
/// `pages` stores opening cursors through the shown page (`None` for the first).
/// Failed requests preserve both the shown page and every pending navigation click.
#[derive(Debug)]
struct Pager {
    pages: Vec<Option<String>>,
    next: Option<String>,
    requested: Option<usize>,
}

impl Default for Pager {
    fn default() -> Self {
        Self {
            pages: vec![None],
            next: None,
            requested: None,
        }
    }
}

impl Pager {
    const fn next(&mut self) {
        let requested = match self.requested {
            Some(requested) => requested,
            None => self.pages.len(),
        };
        if requested < self.pages.len() || self.next.is_some() {
            self.requested = Some(requested.saturating_add(1));
        }
    }

    fn previous(&mut self) {
        self.requested = Some(
            self.requested
                .unwrap_or(self.pages.len())
                .saturating_sub(1)
                .max(1),
        );
    }

    fn refresh(&mut self, api: &Api) -> Result<Event, ApiError> {
        let mut requested = self.requested.unwrap_or(self.pages.len());
        let (mut page, forward) = if requested > self.pages.len() {
            if let Some(cursor) = &self.next {
                let (page, forward) =
                    Self::fetch_forward(api, cursor, requested - self.pages.len())?;
                requested = self.pages.len() + forward.len() + 1;
                (page, forward)
            } else {
                requested = self.pages.len();
                (
                    api.documents(self.pages.last().and_then(Option::as_deref))?,
                    Vec::new(),
                )
            }
        } else {
            (
                api.documents(self.pages.get(requested - 1).and_then(Option::as_deref))?,
                Vec::new(),
            )
        };
        if page.items.is_empty() && requested > 1 {
            // Everything from this page on was printed or queued: start over.
            page = api.documents(None)?;
            requested = 1;
        }
        if requested > self.pages.len() {
            self.pages.push(self.next.take());
            self.pages.extend(forward);
        } else {
            self.pages.truncate(requested);
        }
        self.next = page.next_cursor;
        self.requested = None;
        Ok(Event::Documents {
            items: page.items,
            page: self.pages.len(),
            has_next: self.next.is_some(),
        })
    }

    fn fetch_forward(
        api: &Api,
        cursor: &str,
        count: usize,
    ) -> Result<(Page, Vec<Option<String>>), ApiError> {
        let mut page = api.documents(Some(cursor))?;
        let mut opened = Vec::new();
        while opened.len() + 1 < count && !page.items.is_empty() {
            let Some(cursor) = page.next_cursor.take() else {
                break;
            };
            page = api.documents(Some(&cursor))?;
            opened.push(Some(cursor));
        }
        Ok((page, opened))
    }
}

#[cfg(test)]
mod tests;
