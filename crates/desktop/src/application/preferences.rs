//! Serializes GUI preferences and redacted status writes; credentials remain worker-owned.
use std::{
    io,
    sync::{
        Arc,
        mpsc::{self, Sender},
    },
    thread,
};

use agent::{Error, settings::Settings, status::Snapshot, store::StateDir};
use fiscal::device::Vendor;

use crate::autostart;

pub(super) enum Request {
    Save { settings: Settings, start: bool },
    Telemetry(bool),
    Port { port: String, vendor: Vendor },
    Autostart { settings: Settings, enabled: bool },
    Reload,
    Status(Snapshot),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Saved,
    Start(Settings),
    Telemetry,
    Port,
    Autostart(bool),
    Reload(Settings),
    Status(Snapshot),
}

pub(super) type Completion = Result<Outcome, String>;

pub(super) struct Writer {
    sender: Sender<Request>,
}

impl Writer {
    pub(super) fn spawn<S>(dir: Arc<StateDir>, sink: S) -> io::Result<Self>
    where
        S: Fn(Completion) + Send + 'static,
    {
        let (sender, receiver) = mpsc::channel();
        let owner = thread::Builder::new()
            .name("application settings".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    sink(persist(&dir, request).map_err(|error| error.to_string()));
                }
            })?;
        // Pending writes are acknowledged before quitting; the idle receiver need not be joined.
        drop(owner);
        Ok(Self { sender })
    }

    /// A writer whose thread is gone, so every submission fails.
    #[cfg(test)]
    pub(super) fn closed() -> Self {
        Self {
            sender: mpsc::channel().0,
        }
    }

    pub(super) fn submit(&self, request: Request) -> Result<(), mpsc::SendError<Request>> {
        self.sender.send(request)
    }
}

fn persist(dir: &StateDir, request: Request) -> Result<Outcome, Error> {
    match request {
        Request::Save { settings, start } => {
            settings.save(dir)?;
            Ok(if start {
                Outcome::Start(settings)
            } else {
                Outcome::Saved
            })
        },
        Request::Telemetry(enabled) => {
            Settings::save_telemetry_enabled(dir, enabled)?;
            Ok(Outcome::Telemetry)
        },
        Request::Port { port, vendor } => {
            if let Some(mut settings) = Settings::load(dir)? {
                settings.port = port;
                settings.vendor = Some(vendor);
                settings.save(dir)?;
            }
            Ok(Outcome::Port)
        },
        Request::Autostart { settings, enabled } => {
            let settings = Settings::load(dir)?.unwrap_or(settings);
            autostart::configure(dir, settings, enabled)?;
            Ok(Outcome::Autostart(enabled))
        },
        Request::Reload => Ok(Outcome::Reload(Settings::load(dir)?.unwrap_or_default())),
        Request::Status(snapshot) => {
            snapshot.save(dir)?;
            Ok(Outcome::Status(snapshot))
        },
    }
}

#[cfg(test)]
#[path = "preferences/tests.rs"]
mod tests;
