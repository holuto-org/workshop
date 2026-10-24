//! Sole thread for printer I/O and Holuto requests.
//! GUI commands run between polls; receipts and reports never interleave.
//! Cooperative shutdown finishes the current operation before stopping.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use jiff::civil::Date;
use posnet::{
    printer::{Printer, Report, Status},
    session::{Connection, Link},
};

use crate::{
    Error,
    agent::{Agent, Blocked, RETRY},
    api::{Api, ApiError, Holuto, queue_form},
    documents::{BuyerNip, Document},
    journal::Journal,
    settings::Settings,
    store::StateDir,
};

#[derive(Clone, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub enum Event {
    Status {
        text: String,
        tone: Tone,
    },
    /// Detection may change and persist `port`.
    Connected {
        serial: String,
        port: String,
    },
    /// `page` is 1-based.
    Documents {
        items: Vec<Document>,
        page: usize,
        has_next: bool,
    },
    Device {
        status: Option<Status>,
        blocked: bool,
        report_unresolved: bool,
    },
    HistoryChanged,
    /// Safe to join the worker.
    Stopped,
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
    pub fn spawn<S>(
        dir: Arc<StateDir>,
        settings: Settings,
        link: Arc<dyn Link>,
        sink: S,
    ) -> Result<Self, Error>
    where
        S: Fn(Event) + Send + Sync + 'static,
    {
        let (messages, inbox) = mpsc::channel();
        let path = dir.path().to_owned();
        let thread = thread::Builder::new()
            .name("printer worker".to_owned())
            .spawn(move || {
                if let Err(error) = run(&dir, &settings, link, &inbox, &sink) {
                    sink(Event::Status {
                        text: format!("Błąd: {error}"),
                        tone: Tone::Error,
                    });
                }
                sink(Event::Stopped);
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
    // Unresolved operations forbid switching devices or scanning other ports.
    let may_switch = Journal::new(dir)
        .load()?
        .is_none_or(|operation| !operation.blocks());
    let mut printer = Printer::new(
        link,
        Connection {
            port: settings.port.clone(),
            baud_rate: settings.baud_rate,
        },
    );
    let status = printer.detect(may_switch)?;
    let port = printer.connection().port.clone();
    if port != settings.port {
        Settings::remember_port(dir, &port)?;
    }
    let api = Api::new(&settings.api_url, &settings.token, &status.serial)?;
    let waiting = |problem: Option<&str>| {
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
    agent.session()?;
    sink(Event::Connected {
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
        queue: VecDeque::new(),
        sink,
    }
    .run(inbox)
}

struct Session<'a, H> {
    agent: Agent<'a, H>,
    pager: Pager,
    queue: VecDeque<Command>,
    sink: &'a dyn Fn(Event),
}

impl<H: Holuto> Session<'_, H> {
    fn run(mut self, inbox: &Receiver<Message>) -> Result<(), Error> {
        loop {
            if self.drain(inbox) {
                return Ok(());
            }
            let delay = match self.poll(inbox) {
                Ok(Some(delay)) => delay,
                Ok(None) => return Ok(()),
                Err(error @ Error::Api(ApiError::Unauthorized)) => return Err(error),
                Err(error) => {
                    self.say(format!("Błąd: {error}"), Tone::Error);
                    self.publish_device();
                    RETRY
                },
            };
            match inbox.recv_timeout(delay) {
                Ok(Message::Command(command)) => self.queue.push_back(command),
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {},
            }
        }
    }

    /// Drains commands; returns whether stop was requested.
    fn drain(&mut self, inbox: &Receiver<Message>) -> bool {
        while let Ok(message) = inbox.try_recv() {
            match message {
                Message::Command(command) => self.queue.push_back(command),
                Message::Stop => return true,
            }
        }
        false
    }

    fn poll(&mut self, inbox: &Receiver<Message>) -> Result<Option<Duration>, Error> {
        while let Some(command) = self.queue.pop_front() {
            let done = self.command(command);
            self.publish_news();
            done?;
            if self.drain(inbox) {
                return Ok(None);
            }
        }
        let delay = self.agent.step();
        self.publish_news();
        if self.drain(inbox) {
            return Ok(None);
        }
        // Printer and settlement errors must not prevent fetching Holuto documents.
        let refreshed = self
            .pager
            .refresh(self.agent.api())
            .map(|page| (self.sink)(page));
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
                return Ok(());
            },
            Command::PreviousPage => {
                self.pager.previous();
                return Ok(());
            },
            Command::ResolveReport => {
                self.agent.resolve_report()?;
                self.publish_device();
                self.say("Wyjaśniono raport.".to_owned(), Tone::Success);
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
        let blocked = self.agent.blocked();
        let report_unresolved = matches!(blocked, Ok(Some(Blocked::Report)));
        (self.sink)(Event::Device {
            status: self.agent.status().cloned(),
            // An unreadable journal also blocks printing.
            blocked: !matches!(blocked, Ok(None)),
            report_unresolved,
        });
    }

    /// Publish even if the call that produced the news failed.
    fn publish_news(&mut self) {
        let news = self.agent.take_news();
        for notice in news.notices {
            self.say(notice, Tone::Error);
        }
        if news.history_changed {
            (self.sink)(Event::HistoryChanged);
        }
    }

    fn say(&self, text: String, tone: Tone) {
        (self.sink)(Event::Status { text, tone });
    }
}

/// Fetches one bounded page per poll.
/// `pages` stores opening cursors through the shown page (`None` for the first).
/// Failed requests preserve the shown page and its cursor.
#[derive(Debug)]
struct Pager {
    pages: Vec<Option<String>>,
    next: Option<String>,
    requested: Option<Vec<Option<String>>>,
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
    fn next(&mut self) {
        if let Some(cursor) = &self.next {
            let mut pages = self.pages.clone();
            pages.push(Some(cursor.clone()));
            self.requested = Some(pages);
        }
    }

    fn previous(&mut self) {
        let mut pages = self.pages.clone();
        let _shown = pages.pop();
        self.requested = Some(if pages.is_empty() { vec![None] } else { pages });
    }

    fn refresh(&mut self, api: &mut impl Holuto) -> Result<Event, ApiError> {
        let mut pages = self.requested.take().unwrap_or_else(|| self.pages.clone());
        let mut page = api.documents(pages.last().and_then(Option::as_deref))?;
        if page.items.is_empty() && pages.len() > 1 {
            // Everything from this page on was printed or queued: start over.
            pages = vec![None];
            page = api.documents(None)?;
        }
        self.next.clone_from(&page.next_cursor);
        self.pages = pages;
        Ok(Event::Documents {
            items: page.items,
            page: self.pages.len(),
            has_next: self.next.is_some(),
        })
    }
}

#[cfg(test)]
mod tests;
