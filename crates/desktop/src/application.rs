use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::Arc,
};

use agent::{
    history,
    settings::Settings,
    store::StateDir,
    worker::{Command, Event, Tone, Worker},
};
use posnet::session::{Link, serial_ports};
use slint::{CloseRequestResponse, ComponentHandle};

use crate::{
    model::{Model, strings},
    tray,
    ui::{Action, AppWindow, Tab, TrayIcon},
};

thread_local! {
    // Worker callbacks carry only owned events across threads, never GUI handles.
    static APPLICATION: RefCell<Option<Rc<RefCell<Application>>>> = const { RefCell::new(None) };
}

struct Application {
    model: Model,
    dir: Arc<StateDir>,
    link: Arc<dyn Link>,
    worker: Option<Worker>,
    window: Option<AppWindow>,
    tray: Option<TrayIcon>,
    quitting: bool,
}

pub(crate) fn run(
    dir: Arc<StateDir>,
    settings: Settings,
    link: Arc<dyn Link>,
) -> Result<(), slint::PlatformError> {
    let app = Rc::new(RefCell::new(Application {
        model: Model::new(settings),
        dir,
        link,
        worker: None,
        window: None,
        tray: None,
        quitting: false,
    }));
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    let tray = TrayIcon::new()?;
    let weak = Rc::downgrade(&app);
    tray.on_open(move || with_app(&weak, Application::open));
    let weak = Rc::downgrade(&app);
    tray.on_quit(move || with_app(&weak, |app| app.stop(true)));
    match tray.show() {
        Ok(()) => app.borrow_mut().tray = Some(tray),
        Err(error) => app.borrow_mut().model.say(
            format!("Zasobnik systemowy jest niedostępny: {error}"),
            Tone::Error,
        ),
    }
    app.borrow_mut().reload_history();
    app.borrow_mut().open()?;
    let result = slint::run_event_loop_until_quit();
    APPLICATION.with(|slot| *slot.borrow_mut() = None);
    result
}

fn with_app<F>(weak: &Weak<RefCell<Application>>, callback: F)
where
    F: FnOnce(&mut Application) -> Result<(), slint::PlatformError>,
{
    if let Some(app) = weak.upgrade() {
        let mut app = app.borrow_mut();
        if let Err(error) = callback(&mut app) {
            app.model.say(error.to_string(), Tone::Error);
        }
        app.sync();
    }
}

impl Application {
    fn sync(&self) {
        if let Some(window) = &self.window {
            self.model.sync(window);
        }
    }

    fn capture(&mut self) {
        if let Some(window) = &self.window {
            self.model.capture(window);
        }
    }

    fn open(&mut self) -> Result<(), slint::PlatformError> {
        self.capture();
        if let Some(window) = &self.window {
            window.show()?;
            window.window().set_minimized(false);
            window.window().request_redraw();
            return Ok(());
        }
        let window = AppWindow::new()?;
        let weak = APPLICATION
            .with(|slot| slot.borrow().as_ref().map(Rc::downgrade))
            .unwrap_or_default();
        let action_weak = Weak::clone(&weak);
        window.on_action(move |action| {
            with_app(&action_weak, |app| {
                app.capture();
                app.action(action);
                Ok(())
            });
        });
        let select_weak = Weak::clone(&weak);
        window.on_selected_document(move |index| {
            with_app(&select_weak, |app| {
                app.capture();
                app.model.selected = usize::try_from(index)
                    .ok()
                    .filter(|index| *index < app.model.documents.len());
                Ok(())
            });
        });
        let tab_weak = Weak::clone(&weak);
        window.on_tab_changed(move |tab| {
            with_app(&tab_weak, |app| {
                app.capture();
                app.model.tab = tab;
                if tab == Tab::History {
                    app.reload_history();
                }
                Ok(())
            });
        });
        let date_weak = Weak::clone(&weak);
        window.on_date_changed(move |from, date| {
            with_app(&date_weak, |app| {
                app.capture();
                app.model.change_date(from, &date);
                Ok(())
            });
        });
        window.on_confirm(move |accepted| {
            with_app(&weak, |app| {
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
        window.window().on_close_requested(request_close);
        window.set_ports(strings(serial_ports()));
        self.model.initialize(&window);
        window.show()?;
        self.window = Some(window);
        Ok(())
    }

    fn save(&mut self) -> bool {
        match self.model.settings.save(&self.dir) {
            Ok(()) => {
                self.model.say("Zapisano ustawienia.".into(), Tone::Success);
                true
            },
            Err(error) => {
                self.model.say(error.to_string(), Tone::Error);
                false
            },
        }
    }

    fn action(&mut self, action: Action) {
        match action {
            Action::Save => {
                if self.worker.is_none() {
                    let _saved = self.save();
                }
            },
            Action::Start => self.start(),
            Action::Stop => {
                if let Err(error) = self.stop(false) {
                    self.model.say(error.to_string(), Tone::Error);
                }
            },
            Action::Quit => {
                if let Err(error) = self.stop(true) {
                    self.model.say(error.to_string(), Tone::Error);
                }
            },
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
            | Action::ResolveReport => self.model.request(action),
        }
    }

    fn start(&mut self) {
        if self.worker.is_some() || !self.save() {
            return;
        }
        let sink = |event| {
            let _event_loop_closed = slint::invoke_from_event_loop(move || {
                APPLICATION.with(|slot| {
                    if let Some(app) = slot.borrow().as_ref() {
                        let mut app = app.borrow_mut();
                        app.capture();
                        app.event(event);
                        app.sync();
                    }
                });
            });
        };
        match Worker::spawn(
            Arc::clone(&self.dir),
            self.model.settings.clone(),
            Arc::clone(&self.link),
            sink,
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
                self.model.running = true;
                self.model.stopping = false;
                self.model
                    .say("Łączenie z drukarką i Holuto…".into(), Tone::Info);
            },
            Err(error) => self.model.say(error.to_string(), Tone::Error),
        }
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
        } else if self.quitting {
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
        match event {
            Event::Status { text, tone } => self.model.say(text, tone),
            Event::Connected { serial, port } => {
                self.model.connected = true;
                self.model.serial = serial;
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
            } => {
                self.model.device = status;
                self.model.blocked = blocked;
                self.model.report_unresolved = report_unresolved;
            },
            Event::HistoryChanged => self.reload_history(),
            Event::Stopped => {
                // This event is emitted only after the worker has finished its operation.
                if let Some(worker) = self.worker.take() {
                    worker.join();
                }
                self.model.running = false;
                self.model.stopping = false;
                self.model.connected = false;
                self.reload_history();
                if self.quitting
                    && let Err(error) = slint::quit_event_loop()
                {
                    self.model.say(error.to_string(), Tone::Error);
                }
            },
        }
    }
}

fn request_close() -> CloseRequestResponse {
    // D-Bus is asynchronous; the window stays visible until a tray host is confirmed.
    let check = slint::spawn_local(async {
        let available = tray::available().await;
        APPLICATION.with(|slot| {
            if let Some(app) = slot.borrow().as_ref() {
                let mut app = app.borrow_mut();
                app.capture();
                if available && app.tray.is_some() {
                    if let Some(window) = app.window.take()
                        && let Err(error) = window.hide()
                    {
                        app.model.say(error.to_string(), Tone::Error);
                        app.window = Some(window);
                    }
                } else {
                    app.model.say(
                        concat!(
                            "Zasobnik systemowy jest niedostępny. ",
                            "Aby zakończyć aplikację, kliknij Zakończ."
                        )
                        .into(),
                        Tone::Error,
                    );
                }
                app.sync();
            }
        });
    });
    if let Err(error) = check {
        APPLICATION.with(|slot| {
            if let Some(app) = slot.borrow().as_ref() {
                let mut app = app.borrow_mut();
                app.model.say(error.to_string(), Tone::Error);
                app.sync();
            }
        });
    }
    CloseRequestResponse::KeepWindowShown
}
