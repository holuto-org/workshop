//! Serializes GUI preferences and redacted status writes; credentials remain worker-owned.
use std::{
    io,
    sync::{
        Arc,
        mpsc::{self, Sender},
    },
    thread,
};

use agent::{
    Error,
    settings::{Settings, Theme},
    status::Snapshot,
    store::StateDir,
};
use fiscal::device::Vendor;
use i18n::{LanguagePreference, Message};

use crate::autostart;

pub(super) enum Request {
    Save {
        settings: Settings,
        start: bool,
    },
    Telemetry(bool),
    Theme(Theme),
    Language(LanguagePreference),
    Port {
        port: String,
        vendor: Vendor,
    },
    Autostart {
        telemetry_enabled: bool,
        enabled: bool,
    },
    Reload,
    Status(Snapshot),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Saved(Settings),
    Start(Settings),
    Telemetry,
    Theme,
    Language,
    Port,
    Autostart(bool),
    Reload(Settings),
    Status(Snapshot),
}

pub(super) type Completion = Result<Outcome, Message>;

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
                    sink(persist(&dir, request).map_err(|error| error.message()));
                }
            })?;
        // Quit waits for write acknowledgements, not this idle receiver.
        drop(owner);
        Ok(Self { sender })
    }

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
        Request::Save {
            mut settings,
            start,
        } => {
            // The window's token can predate pairing (a failed reload); the stored one decides.
            let mut stored = Settings::load(dir)?.unwrap_or_default();
            stored.set_api_url(&settings.api_url);
            settings.token = stored.token;
            if start {
                settings.validate()?;
            }
            settings.save(dir)?;
            Ok(if start {
                Outcome::Start(settings)
            } else {
                Outcome::Saved(settings)
            })
        },
        Request::Telemetry(enabled) => {
            Settings::save_telemetry_enabled(dir, enabled)?;
            Ok(Outcome::Telemetry)
        },
        Request::Theme(theme) => {
            Settings::save_theme(dir, theme)?;
            Ok(Outcome::Theme)
        },
        Request::Language(language) => {
            Settings::save_language(dir, language)?;
            Ok(Outcome::Language)
        },
        Request::Port { port, vendor } => {
            if let Some(mut settings) = Settings::load(dir)? {
                settings.port = port;
                settings.vendor = Some(vendor);
                settings.save(dir)?;
            }
            Ok(Outcome::Port)
        },
        Request::Autostart {
            telemetry_enabled,
            enabled,
        } => {
            // Unsaved connection edits stay in the window; diagnostics is already a live choice.
            let settings = Settings::load(dir)?.unwrap_or_else(|| Settings {
                telemetry_enabled,
                ..Settings::default()
            });
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
