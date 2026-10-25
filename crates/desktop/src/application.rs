#[cfg(target_os = "macos")]
#[path = "application/macos.rs"]
pub(crate) mod macos;
#[path = "application/preferences.rs"]
mod preferences;

use std::{cell::RefCell, collections::VecDeque, fmt::Display, rc::Rc, sync::Arc};

use agent::{
    history,
    settings::Settings,
    status::Snapshot,
    store::StateDir,
    worker::{self, Command, Event, Tone, Worker},
};
use fiscal::link::{Link, serial_ports};
use jiff::{Zoned, civil::Date};
use slint::{
    CloseRequestResponse, ComponentHandle,
    winit_030::{WinitWindowAccessor as _, winit::window::Window as NativeWindow},
};

use crate::{
    model::{Model, Tab, strings},
    telemetry, tray,
    ui::{Action, AppWindow, TrayIcon},
};

#[cfg(target_os = "macos")]
const NATIVE_QUIT_ERROR: &str = concat!(
    "Nie można bezpiecznie obsłużyć zamykania aplikacji w macOS. ",
    "Obsługa drukarki jest wyłączona."
);

thread_local! {
    // Cross-thread callbacks carry owned events, never GUI handles.
    static APPLICATION: RefCell<Option<Rc<RefCell<Application>>>> = const { RefCell::new(None) };
    static CALLBACKS: RefCell<VecDeque<AppCallback>> = const { RefCell::new(VecDeque::new()) };
}

type AppCallback = Box<dyn FnOnce(&mut Application) -> Result<(), slint::PlatformError>>;

enum WriteKind {
    Settings,
    Status,
}

struct Application {
    model: Model,
    dir: Arc<StateDir>,
    link: Arc<dyn Link>,
    worker: Option<Worker>,
    window: Option<AppWindow>,
    tray: Option<TrayIcon>,
    quitting: bool,
    settings_writer: Option<preferences::Writer>,
    writes_pending: VecDeque<WriteKind>,
    status_requested: Option<Snapshot>,
    /// Latest settled status, including failed writes so they cannot prevent quitting.
    status_settled: Option<Snapshot>,
}

pub(crate) fn run(
    dir: Arc<StateDir>,
    settings: Settings,
    link: Arc<dyn Link>,
) -> Result<(), slint::PlatformError> {
    let app = Rc::new(RefCell::new(Application::new(
        dir,
        settings,
        link,
        Zoned::now().date(),
    )));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    let tray = TrayIcon::new()?;
    tray.on_open(|| with_app(Application::open));
    tray.on_quit(|| with_app(|app| app.stop(true)));
    // Only sets `visible`; Slint creates the native icon later and cannot report failure,
    // so closing asks `tray::available()` before hiding.
    tray.show()?;
    app.borrow_mut().tray = Some(tray);
    app.borrow_mut().launch()?;
    let result = slint::run_event_loop_until_quit();
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    result
}

fn with_app<F>(callback: F)
where
    F: FnOnce(&mut Application) -> Result<(), slint::PlatformError> + 'static,
{
    CALLBACKS.with(|callbacks| callbacks.borrow_mut().push_back(Box::new(callback)));
    let Some(app) = APPLICATION.with(|slot| slot.borrow().as_ref().map(Rc::clone)) else {
        return;
    };
    loop {
        // Re-entrant sync/show callbacks queue work for the outer drain.
        let Ok(mut state) = app.try_borrow_mut() else {
            return;
        };
        let Some(callback) = CALLBACKS.with(|callbacks| callbacks.borrow_mut().pop_front()) else {
            return;
        };
        if let Err(error) = callback(&mut state) {
            state.model.say(error.to_string(), Tone::Error);
        }
        state.sync();
    }
}

impl Application {
    fn new(dir: Arc<StateDir>, settings: Settings, link: Arc<dyn Link>, today: Date) -> Self {
        Self {
            model: Model::new(settings, today),
            dir,
            link,
            worker: None,
            window: None,
            tray: None,
            quitting: false,
            settings_writer: None,
            writes_pending: VecDeque::new(),
            status_requested: None,
            status_settled: None,
        }
    }

    fn launch(&mut self) -> Result<(), slint::PlatformError> {
        self.reload_history();
        // A block outlives restarts; `workshop status` must show it before Start.
        let block = worker::journal_block(&self.dir);
        self.model.blocked = block.blocked;
        self.model.report_unresolved = block.report_unresolved;
        if block.blocked {
            // Settings confirmations stay hidden while blocked; say why nothing may print.
            self.model.say(
                "Poprzednia operacja jest nierozstrzygnięta. Uruchom obsługę drukarki, aby ją \
                 wyjaśnić."
                    .into(),
                Tone::Error,
            );
        }
        self.open()?;
        #[cfg(unix)]
        if let Err(error) = quit_on_session_end() {
            self.model.say(
                format!("Nie można obsłużyć zamykania sesji: {error}"),
                Tone::Error,
            );
        }
        self.publish_status();
        if self.model.settings.start_on_launch {
            self.start();
            self.sync();
        }
        Ok(())
    }

    fn sync(&self) {
        if let Some(window) = &self.window {
            self.model.sync(window);
        }
    }

    fn capture(&mut self) {
        if let Some(window) = &self.window {
            self.model.capture(window, Zoned::now().date());
        }
    }

    fn open(&mut self) -> Result<(), slint::PlatformError> {
        self.capture();
        if let Some(window) = &self.window {
            window.show()?;
            window.window().set_minimized(false);
            let _native_window = window
                .window()
                .with_winit_window(NativeWindow::focus_window);
            window.window().request_redraw();
            return Ok(());
        }
        let window = AppWindow::new()?;
        window.on_action(|action| {
            with_app(move |app| {
                app.capture();
                app.action(action)
            });
        });
        window.on_selected_document(|index| {
            with_app(move |app| {
                app.capture();
                app.model.selected = usize::try_from(index)
                    .ok()
                    .filter(|index| *index < app.model.documents().len());
                Ok(())
            });
        });
        window.on_sort_documents(|column, descending| {
            with_app(move |app| {
                app.capture();
                app.model.sort_documents(column, descending);
                Ok(())
            });
        });
        window.on_sort_history(|column, descending| {
            with_app(move |app| {
                app.capture();
                app.model.sort_history(column, descending);
                Ok(())
            });
        });
        window.on_confirm(|accepted| {
            with_app(move |app| {
                app.capture();
                let worker = app.worker.as_ref();
                app.model.confirm(accepted, |command| {
                    if let Some(worker) = worker {
                        worker.submit(command);
                    }
                });
                Ok(())
            });
        });
        window.on_telemetry_changed(|enabled| {
            with_app(move |app| {
                if app.model.pairing {
                    return Ok(());
                }
                // Opt-out is live even while the fiscal worker is running.
                telemetry::set_enabled(enabled);
                app.capture();
                app.model.settings.telemetry_enabled = enabled;
                let _submitted = app.write_settings(preferences::Request::Telemetry(enabled));
                Ok(())
            });
        });
        window.on_autostart_changed(|enabled| {
            with_app(move |app| {
                if !app.model.pairing && !app.model.saving && !app.quitting {
                    app.capture();
                    let _submitted = app.write_settings(preferences::Request::Autostart {
                        settings: app.model.settings.clone(),
                        enabled,
                    });
                }
                Ok(())
            });
        });
        window.window().on_close_requested(request_close);
        window.set_ports(strings(
            serial_ports().into_iter().map(|candidate| candidate.port),
        ));
        self.model.initialize(&window, Zoned::now().date());
        window.show()?;
        #[cfg(target_os = "macos")]
        if !macos::install() {
            self.model.say(NATIVE_QUIT_ERROR.into(), Tone::Error);
        }
        self.window = Some(window);
        self.sync();
        Ok(())
    }

    fn save(&mut self, start: bool) {
        let validation = if start {
            self.model.settings.validate()
        } else {
            self.model.settings.validate_configuration()
        };
        if let Err(error) = validation {
            self.model.say(error.to_string(), Tone::Error);
            return;
        }
        let _submitted = self.write_settings(preferences::Request::Save {
            settings: self.model.settings.clone(),
            start,
        });
    }

    fn write_settings(&mut self, request: preferences::Request) -> bool {
        let is_status = matches!(&request, preferences::Request::Status(_));
        let writer = match &mut self.settings_writer {
            Some(writer) => Ok(writer),
            slot @ None => {
                let sink = |completion| {
                    let _event_loop_closed = slint::invoke_from_event_loop(move || {
                        with_app(move |app| {
                            app.capture();
                            app.settings_saved(completion);
                            Ok(())
                        });
                    });
                };
                preferences::Writer::spawn(Arc::clone(&self.dir), sink)
                    .map(|writer| slot.insert(writer))
            },
        };
        let description = if is_status {
            "stanu aplikacji"
        } else {
            "ustawień"
        };
        let failed = |error: &dyn Display| format!("Nie można zapisać {description}: {error}");
        let submitted = writer
            .map_err(|error| failed(&error))
            .and_then(|writer| writer.submit(request).map_err(|error| failed(&error)));
        match submitted {
            Ok(()) => {
                self.writes_pending.push_back(if is_status {
                    WriteKind::Status
                } else {
                    WriteKind::Settings
                });
                self.model.saving |= !is_status;
                true
            },
            Err(message) => {
                self.model.say(message, Tone::Error);
                false
            },
        }
    }

    fn publish_status(&mut self) {
        if self
            .status_requested
            .as_ref()
            .is_some_and(|snapshot| self.model.status_matches(snapshot))
        {
            return;
        }
        let snapshot = self.model.status(jiff::Timestamp::now());
        if self.write_settings(preferences::Request::Status(snapshot.clone())) {
            self.status_requested = Some(snapshot);
        } else {
            // Submission failure must allow quit without letting an older acknowledgement win.
            self.status_requested = None;
            self.status_settled = Some(snapshot);
        }
    }

    fn settings_saved(&mut self, completion: preferences::Completion) {
        let kind = self.writes_pending.pop_front();
        self.model.saving = self
            .writes_pending
            .iter()
            .any(|kind| matches!(kind, WriteKind::Settings));
        match completion {
            Ok(preferences::Outcome::Start(mut settings)) if !self.quitting => {
                settings.telemetry_enabled = self.model.settings.telemetry_enabled;
                self.model.settings = settings;
                self.model.settings_persisted();
                self.spawn_worker();
            },
            Ok(preferences::Outcome::Saved) => {
                // Inputs stay locked while saving, so the model still holds what was written.
                self.model.settings_persisted();
                if !self.model.blocked {
                    self.model.say("Zapisano ustawienia.".into(), Tone::Success);
                }
            },
            Ok(preferences::Outcome::Telemetry) if !self.model.blocked => {
                self.model
                    .say("Zapisano ustawienia diagnostyki.".into(), Tone::Success);
            },
            Ok(preferences::Outcome::Autostart(enabled)) => {
                self.model.settings.start_on_launch = enabled;
                if !self.model.blocked {
                    self.model
                        .say("Zapisano ustawienia uruchamiania.".into(), Tone::Success);
                }
            },
            Ok(preferences::Outcome::Reload(mut settings)) => {
                // A diagnostics choice made after the reload was read is written after it.
                settings.telemetry_enabled = self.model.settings.telemetry_enabled;
                self.model.settings = settings;
                self.model.settings_persisted();
            },
            Ok(preferences::Outcome::Status(snapshot))
                if self.status_requested.as_ref() == Some(&snapshot) =>
            {
                self.status_settled = Some(snapshot);
            },
            Ok(
                preferences::Outcome::Port
                | preferences::Outcome::Start(_)
                | preferences::Outcome::Telemetry
                | preferences::Outcome::Status(_),
            ) => {},
            Err(error) => {
                // Forget the request so the next state change retries the write.
                if matches!(kind, Some(WriteKind::Status))
                    && !self
                        .writes_pending
                        .iter()
                        .any(|kind| matches!(kind, WriteKind::Status))
                    && let Some(requested) = self.status_requested.take()
                {
                    self.status_settled = Some(requested);
                }
                self.model.say(error, Tone::Error);
            },
        }
        if let Err(error) = self.finish_quit() {
            self.model.say(error.to_string(), Tone::Error);
        }
    }

    fn action(&mut self, action: Action) -> Result<(), slint::PlatformError> {
        match action {
            Action::Save => {
                if self.worker.is_none() && !self.model.saving && !self.quitting {
                    self.save(false);
                }
            },
            Action::Pair => self.pair(),
            Action::Start => self.start(),
            Action::Stop => return self.stop(false),
            Action::Quit => return self.stop(true),
            Action::PreviousPage => {
                if self.model.connected && self.model.page > 1 {
                    self.submit(Command::PreviousPage);
                }
            },
            Action::NextPage => {
                if self.model.connected && self.model.has_next {
                    self.submit(Command::NextPage);
                }
            },
            Action::QueueReceipt
            | Action::DailyReport
            | Action::MonthlyReport
            | Action::PeriodicReport
            | Action::ResolveReport
            | Action::AcknowledgeOrphanedReceipt => self.model.request(action),
        }
        Ok(())
    }

    fn pair(&mut self) {
        if self.worker.is_some() || self.model.saving || self.quitting {
            return;
        }
        #[cfg(target_os = "macos")]
        if !macos::install() {
            self.model.say(NATIVE_QUIT_ERROR.into(), Tone::Error);
            return;
        }
        match Worker::pair(
            Arc::clone(&self.dir),
            self.model.settings.clone(),
            self.model.pairing_code.clone(),
            Self::worker_event,
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.model.pairing = true;
                self.model.stopping = false;
                self.model.say("Parowanie z Holuto…".into(), Tone::Info);
            },
            Err(error) => self.model.say(error.to_string(), Tone::Error),
        }
    }

    fn start(&mut self) {
        if self.worker.is_none() && !self.model.saving && !self.quitting {
            self.save(true);
        }
    }

    fn spawn_worker(&mut self) {
        #[cfg(target_os = "macos")]
        if !macos::install() {
            self.model.say(NATIVE_QUIT_ERROR.into(), Tone::Error);
            return;
        }
        match Worker::spawn(
            Arc::clone(&self.dir),
            self.model.settings.clone(),
            Arc::clone(&self.link),
            Self::worker_event,
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.model.running = true;
                self.model.stopping = false;
                self.model
                    .say("Łączenie z drukarką i Holuto…".into(), Tone::Info);
                self.publish_status();
            },
            Err(error) => self.model.say(error.to_string(), Tone::Error),
        }
    }

    fn worker_event(event: Event) {
        // Tag the worker's Sentry hub before dispatching to the GUI's separate hub.
        if let Event::Connected {
            printer_id,
            unique_number,
            ..
        } = &event
        {
            telemetry::printer(printer_id, unique_number);
        }
        let _event_loop_closed = slint::invoke_from_event_loop(move || {
            with_app(move |app| {
                app.capture();
                app.event(event);
                Ok(())
            });
        });
    }

    fn submit(&self, command: Command) {
        if let Some(worker) = &self.worker {
            worker.submit(command);
        }
    }

    fn stop(&mut self, quit: bool) -> Result<(), slint::PlatformError> {
        self.quitting |= quit;
        if let Some(worker) = &self.worker {
            worker.stop();
            self.model.stopping = true;
            self.model.say(
                "Zatrzymywanie po zakończeniu bieżącej operacji…".into(),
                Tone::Info,
            );
        } else {
            self.publish_status();
            self.finish_quit()?;
        }
        Ok(())
    }

    fn finish_quit(&self) -> Result<(), slint::PlatformError> {
        if self.quitting
            && self.worker.is_none()
            && self.writes_pending.is_empty()
            && self.status_settled.as_ref().is_some_and(|settled| {
                !settled.processing && !settled.connected && self.model.status_matches(settled)
            })
        {
            slint::quit_event_loop().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn reload_history(&mut self) {
        match history::load(&self.dir) {
            Ok(history) => self.model.set_history(history),
            Err(error) => self
                .model
                .say(format!("Nie można odczytać historii: {error}"), Tone::Error),
        }
    }

    fn event(&mut self, event: Event) {
        let publish = matches!(
            &event,
            Event::Connected { .. } | Event::Device { .. } | Event::Stopped
        );
        match event {
            Event::Status { text, tone } => self.model.say(text, tone),
            Event::PairingFailed { message, .. } => self.model.say(message, Tone::Error),
            Event::Paired { .. } => {
                self.model.pairing_code.clear();
                self.model.say(
                    "Sparowano z Holuto. Możesz uruchomić obsługę drukarki.".into(),
                    Tone::Success,
                );
                let _submitted = self.write_settings(preferences::Request::Reload);
            },
            Event::PortDetected { port, vendor } => {
                self.model.settings.port.clone_from(&port);
                self.model.settings.vendor = Some(vendor);
                let _submitted = self.write_settings(preferences::Request::Port { port, vendor });
            },
            Event::Connected {
                printer_id,
                unique_number,
                port,
            } => {
                telemetry::printer(&printer_id, &unique_number);
                self.model.connected = true;
                self.model.unique_number = unique_number;
                self.model.printer_id = Some(printer_id);
                self.model.settings.port = port;
                self.model.tab = Tab::Documents;
            },
            Event::Documents {
                items,
                page,
                has_next,
            } => self.model.set_documents(items, page, has_next),
            Event::Device {
                status,
                blocked,
                report_unresolved,
                orphaned_receipt,
            } => {
                if let Some(device) = &status {
                    self.model.vendor = Some(device.vendor);
                }
                self.model.device = status;
                self.model.blocked = blocked;
                self.model.report_unresolved = report_unresolved;
                self.model.orphaned_receipt = orphaned_receipt;
            },
            Event::HistoryChanged(history) => self.model.set_history(history),
            Event::CommandCompleted => self.model.command_completed(),
            Event::Stopped => {
                // Operation completion precedes Stopped; joining cannot interrupt printer I/O.
                if let Some(worker) = self.worker.take() {
                    worker.join();
                }
                telemetry::clear_printer();
                // Pairing and worker failures already reported their outcome.
                if self.model.stopping && !self.model.pairing && self.model.tone != Tone::Error {
                    self.model.say("Zatrzymano.".into(), Tone::Info);
                }
                self.model.stopped();
                self.model.pairing = false;
            },
        }
        if publish {
            self.publish_status();
            if let Err(error) = self.finish_quit() {
                self.model.say(error.to_string(), Tone::Error);
            }
        }
    }

    fn close_after_probe(&mut self, available: bool) {
        self.capture();
        if available && self.tray.is_some() {
            if let Some(window) = self.window.take()
                && let Err(error) = window.hide()
            {
                self.model.say(error.to_string(), Tone::Error);
                self.window = Some(window);
            }
        } else {
            self.model.say(
                "Zasobnik systemowy jest niedostępny. Aby zakończyć aplikację, kliknij Zakończ."
                    .into(),
                Tone::Error,
            );
        }
    }
}

/// Quits cooperatively on logout, shutdown, `kill` and Ctrl+C.
///
/// `stop` lets the current operation finish instead of the default action killing
/// the worker. Handlers are registered before returning, so no later signal falls
/// back to the default action.
#[cfg(unix)]
fn quit_on_session_end() -> Result<(), String> {
    use async_signal::{Signal, Signals};
    use futures_lite::StreamExt as _;

    let mut signals = Signals::new([Signal::Term, Signal::Hup, Signal::Int])
        .map_err(|error| error.to_string())?;
    let _listening = slint::spawn_local(async move {
        while let Some(received) = signals.next().await {
            if received.is_ok() {
                with_app(|app| app.stop(true));
            }
        }
    })
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn request_close() -> CloseRequestResponse {
    let check = slint::spawn_local(async {
        let available = tray::available().await;
        with_app(move |app| {
            app.close_after_probe(available);
            Ok(())
        });
    });
    if let Err(error) = check {
        with_app(move |app| {
            app.model.say(error.to_string(), Tone::Error);
            Ok(())
        });
    }
    CloseRequestResponse::KeepWindowShown
}

#[cfg(test)]
#[path = "application/tests.rs"]
mod tests;
