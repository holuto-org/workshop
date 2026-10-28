#[cfg(target_os = "macos")]
#[path = "application/macos.rs"]
pub(crate) mod macos;
#[path = "application/preferences.rs"]
mod preferences;

use std::{cell::RefCell, collections::VecDeque, fmt::Display, mem, rc::Rc, sync::Arc};

use agent::{
    settings::Settings,
    status::Snapshot,
    store::StateDir,
    worker::{self, Command, Event, Worker},
};
use fiscal::link::Link;
use jiff::{Zoned, civil::Date};
use slint::{
    CloseRequestResponse, ComponentHandle, Model as _, SharedString,
    winit_030::{WinitWindowAccessor as _, winit::window::Window as NativeWindow},
};

use crate::{
    model::{Model, Tab, strings},
    telemetry, tray,
    ui::{Action, AppWindow, TrayIcon},
};
/// The first port choice; it stores an empty port, which the worker probes.
const AUTO_PORT: &str = "Automatyczne wykrywanie";

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
    /// Detected candidates, refreshed when the window opens or Ustawienia is shown.
    ports: Vec<String>,
    settings_writer: Option<preferences::Writer>,
    writes_pending: VecDeque<WriteKind>,
    /// Latest submitted status, so unchanged state is not written again.
    status_requested: Option<Snapshot>,
    /// The local date for month lists and report ranges; fixed in tests.
    today: fn() -> Date,
}

pub(crate) fn run(
    dir: Arc<StateDir>,
    settings: Settings,
    link: Arc<dyn Link>,
) -> Result<(), slint::PlatformError> {
    let app = Rc::new(RefCell::new(Application::new(dir, settings, link, || {
        Zoned::now().date()
    })));
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
    app.borrow_mut().finish_after_event_loop();
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
            state.model.problem(error.to_string());
        }
        state.sync();
    }
}

impl Application {
    fn new(
        dir: Arc<StateDir>,
        settings: Settings,
        link: Arc<dyn Link>,
        today: fn() -> Date,
    ) -> Self {
        Self {
            model: Model::new(settings, today()),
            dir,
            link,
            worker: None,
            window: None,
            tray: None,
            quitting: false,
            ports: Vec::new(),
            settings_writer: None,
            writes_pending: VecDeque::new(),
            status_requested: None,
            today,
        }
    }

    fn launch(&mut self) -> Result<(), slint::PlatformError> {
        // A block outlives restarts; `workshop status` must show it before Start.
        let block = worker::journal_block(&self.dir);
        self.model.blocked = block.blocked;
        self.model.report_unresolved = block.report_unresolved;
        if block.blocked {
            // Settings confirmations stay hidden while blocked; say why nothing may print.
            self.model.problem(
                "Poprzednia operacja jest nierozstrzygnięta. Uruchom obsługę drukarki, aby ją \
             wyjaśnić."
                    .into(),
            );
        }
        self.open()?;
        #[cfg(unix)]
        if let Err(error) = quit_on_session_end() {
            self.model
                .problem(format!("Nie można obsłużyć zamykania sesji: {error}"));
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
            // The saved port is always offered: enumeration skips unknown adapters it can open.
            let saved = self.model.settings.port.as_str();
            let mut choices = vec![AUTO_PORT.to_owned()];
            if !saved.is_empty() && !self.ports.iter().any(|port| port == saved) {
                choices.push(saved.to_owned());
            }
            choices.extend(self.ports.iter().cloned());
            let index = choices.iter().position(|port| port == saved).unwrap_or(0);
            if window
                .get_ports()
                .iter()
                .ne(choices.iter().map(SharedString::from))
            {
                window.set_ports(strings(choices));
            }
            window.set_port_index(i32::try_from(index).unwrap_or_default());
        }
    }

    fn capture(&mut self) {
        if let Some(window) = &self.window {
            self.model.capture(window, (self.today)());
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
        window.on_selected_receipt(|index| {
            with_app(move |app| {
                app.capture();
                app.model.select_receipt(index);
                Ok(())
            });
        });
        window.on_confirm(|accepted| {
            with_app(move |app| {
                app.capture();
                if let Some(command) = app.model.confirm(accepted) {
                    app.submit(command);
                }
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
                        telemetry_enabled: app.model.settings.telemetry_enabled,
                        enabled,
                    });
                }
                Ok(())
            });
        });
        window.on_refresh_ports(|| {
            with_app(|app| {
                app.capture();
                app.ports = app.ports();
                Ok(())
            });
        });
        window.window().on_close_requested(request_close);
        self.ports = self.ports();
        self.model.initialize(&window, (self.today)());
        window.show()?;
        #[cfg(target_os = "macos")]
        if !macos::install() {
            self.model.problem(NATIVE_QUIT_ERROR.into());
        }
        self.window = Some(window);
        self.sync();
        Ok(())
    }

    /// The worker probes the same candidates; enumeration never opens a port.
    fn ports(&self) -> Vec<String> {
        self.link
            .candidates()
            .into_iter()
            .map(|candidate| candidate.port)
            .collect()
    }

    fn save(&mut self, start: bool) {
        // Start's pairing check needs the stored credential, so the writer validates it.
        if let Err(error) = self.model.settings.validate_configuration() {
            self.model.starting = false;
            self.model.problem(error.to_string());
            return;
        }
        // An unsubmitted write never completes, so it must not leave Start pending.
        if !self.write_settings(preferences::Request::Save {
            settings: self.model.settings.clone(),
            start,
        }) {
            self.model.starting = false;
        }
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
                self.model.problem(message);
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
        // An unsubmitted status is retried on the next state change.
        self.status_requested = self
            .write_settings(preferences::Request::Status(snapshot.clone()))
            .then_some(snapshot);
    }

    fn settings_saved(&mut self, completion: preferences::Completion) {
        let kind = self.writes_pending.pop_front();
        self.model.saving = self
            .writes_pending
            .iter()
            .any(|kind| matches!(kind, WriteKind::Settings));
        match completion {
            Ok(preferences::Outcome::Start(settings)) => {
                self.adopt_settings(settings);
                if mem::take(&mut self.model.starting) {
                    self.spawn_worker();
                }
            },
            Ok(preferences::Outcome::Saved(settings)) => {
                self.adopt_settings(settings);
                if !self.model.blocked {
                    self.model.clear_problem();
                }
            },
            Ok(preferences::Outcome::Telemetry) if !self.model.blocked => {
                self.model.clear_problem();
            },
            Ok(preferences::Outcome::Autostart(enabled)) => {
                self.model.settings.start_on_launch = enabled;
                if !self.model.blocked {
                    self.model.clear_problem();
                }
            },
            Ok(preferences::Outcome::Reload(settings)) => self.adopt_settings(settings),
            Ok(
                preferences::Outcome::Port
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
                {
                    self.status_requested = None;
                }
                // A failed settings write never starts with settings it did not store.
                if matches!(kind, Some(WriteKind::Settings)) {
                    self.model.starting = false;
                }
                self.model.problem(error);
            },
        }
        // Start pressed while a field's save was in flight runs once that save landed.
        if self.model.starting && !self.model.saving && self.worker.is_none() && !self.quitting {
            self.save(true);
        }
        if let Err(error) = self.finish_quit() {
            self.model.problem(error.to_string());
        }
    }

    fn adopt_settings(&mut self, mut settings: Settings) {
        // A diagnostics choice made after the request was queued is written after it.
        settings.telemetry_enabled = self.model.settings.telemetry_enabled;
        self.model.settings = settings;
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
            // Pairing workers take no commands.
            Action::Search | Action::JobsSearch => {
                if self.model.running
                    && let Some(command) = self
                        .model
                        .search_command(matches!(action, Action::JobsSearch))
                {
                    self.submit(command);
                }
            },
            Action::OpenManual => self.model.manual = true,
            Action::CloseManual => self.model.manual = false,
            Action::RetryDocuments => {
                if self.model.connected {
                    self.submit(self.model.retry_documents_command());
                }
            },
            Action::JobsFirstPage => {
                if self.model.connected {
                    self.submit(Command::JobsFirstPage);
                }
            },
            Action::JobsPreviousPage => {
                if self.model.jobs_previous_enabled() {
                    self.submit(Command::JobsPreviousPage);
                }
            },
            Action::JobsNextPage => {
                if self.model.jobs_next_enabled() {
                    self.submit(Command::JobsNextPage);
                }
            },
            Action::PreviousPage => {
                if self.model.connected && self.model.has_previous {
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
            self.model.problem(NATIVE_QUIT_ERROR.into());
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
                self.model.clear_problem();
            },
            Err(error) => self.model.problem(error.to_string()),
        }
    }

    /// A field's save on leaving it can still be in flight; Start then follows it.
    fn start(&mut self) {
        if self.worker.is_none() && !self.model.starting && !self.quitting {
            self.model.starting = true;
            if !self.model.saving {
                self.save(true);
            }
        }
    }

    fn spawn_worker(&mut self) {
        #[cfg(target_os = "macos")]
        if !macos::install() {
            self.model.problem(NATIVE_QUIT_ERROR.into());
            return;
        }
        self.model.clear_problem();
        match Worker::spawn(
            Arc::clone(&self.dir),
            self.model.settings.clone(),
            Arc::clone(&self.link),
            self.model.initial_searches(),
            Self::worker_event,
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.model.running = true;
                self.model.stopping = false;
                self.publish_status();
            },
            Err(error) => self.model.problem(error.to_string()),
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

    /// Tray Quit, signals and macOS termination reach this without a window callback, so
    /// it captures the window before the following sync redraws it.
    fn stop(&mut self, quit: bool) -> Result<(), slint::PlatformError> {
        self.capture();
        self.quitting |= quit;
        if let Some(worker) = &self.worker {
            worker.stop();
            self.model.stopping = true;
            self.model.clear_problem();
        } else {
            self.model.starting = false;
            self.publish_status();
            self.finish_quit()?;
        }
        Ok(())
    }

    fn finish_quit(&self) -> Result<(), slint::PlatformError> {
        // Every status change is published at once, so with no write pending after the
        // worker stopped, the stopped status has been written or has failed.
        if self.quitting && self.worker.is_none() && self.writes_pending.is_empty() {
            // A held AppKit termination ignores the event-loop quit; only its reply exits.
            #[cfg(target_os = "macos")]
            macos::quit();
            slint::quit_event_loop().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn event(&mut self, event: Event) {
        let publish = matches!(
            &event,
            Event::Connected { .. } | Event::Device { .. } | Event::Stopped
        );
        match event {
            Event::Problem(Some(text)) => self.model.problem(text),
            Event::Problem(None) => self.model.clear_problem(),
            Event::PairingFailed { message, .. } => self.model.problem(message),
            Event::Paired { .. } => {
                self.model.pairing_code.clear();
                self.model.clear_problem();
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
                vendor,
                port,
            } => {
                telemetry::printer(&printer_id, &unique_number);
                self.model.connected = true;
                self.model.unique_number = unique_number;
                self.model.vendor = Some(vendor);
                self.model.printer_id = Some(printer_id);
                self.model.settings.port = port;
                self.model.tab = Tab::Receipts;
            },
            Event::Documents {
                items,
                page,
                has_previous,
                has_next,
            } => self
                .model
                .set_documents(items, page, has_previous, has_next),
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
            Event::Jobs {
                items,
                page,
                has_previous,
                has_next,
            } => self.model.set_jobs(items, page, has_previous, has_next),
            Event::JobsUnavailable => self.model.jobs_unavailable(),
            Event::DocumentsUnavailable => self.model.documents_unavailable(),
            Event::CommandCompleted => self.model.command_completed(),
            Event::Stopped => {
                // Operation completion precedes Stopped; joining cannot interrupt printer I/O.
                if let Some(worker) = self.worker.take() {
                    worker.join();
                }
                telemetry::clear_printer();
                self.model.stopped();
                self.model.pairing = false;
            },
        }
        if publish {
            self.publish_status();
            if let Err(error) = self.finish_quit() {
                self.model.problem(error.to_string());
            }
        }
    }

    fn close_after_probe(&mut self, available: bool) {
        self.capture();
        if available && self.tray.is_some() {
            if let Some(window) = self.window.take()
                && let Err(error) = window.hide()
            {
                self.model.problem(error.to_string());
                self.window = Some(window);
            }
        } else {
            self.model.problem(
                "Zasobnik systemowy jest niedostępny. Aby zakończyć aplikację, kliknij Zakończ."
                    .into(),
            );
        }
    }

    /// A lost display server (Wayland logout) ends the event loop without `finish_quit`, so
    /// `Event::Stopped` can no longer arrive. With no GUI left to freeze, stop and wait for the
    /// worker here; `join` returns only after it has emitted `Stopped`.
    fn finish_after_event_loop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
            worker.join();
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
            app.model.problem(error.to_string());
            Ok(())
        });
    }
    CloseRequestResponse::KeepWindowShown
}

#[cfg(test)]
#[path = "application/tests.rs"]
mod tests;
