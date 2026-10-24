#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(all(feature = "simulator", not(debug_assertions)))]
compile_error!("The simulator feature is restricted to local debug builds.");

mod application;
mod model;
mod tray;
#[expect(
    clippy::pub_use,
    clippy::impl_trait_in_params,
    clippy::multiple_inherent_impl,
    clippy::same_name_method,
    reason = "Slint 1.18 emits these items; handwritten Rust keeps workspace lints"
)]
mod ui {
    slint::slint! {
        #[style = "fluent"]
        export * from "../ui/app.slint";
    }
}

use std::{
    error::Error,
    io::{self, Write},
    process::ExitCode,
    sync::Arc,
};

use agent::{settings::Settings, store::StateDir};
use posnet::session::Link;
#[cfg(not(feature = "simulator"))]
use posnet::session::SerialLink;
#[cfg(feature = "simulator")]
use posnet::simulator::Simulator;
use slint::ComponentHandle;

fn main() -> ExitCode {
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Err(dialog_error) = show_error(&error.to_string())
                && let Err(write_error) = writeln!(io::stderr().lock(), "{error}\n{dialog_error}")
            {
                // No output channel remains.
                drop(write_error);
            }
            ExitCode::FAILURE
        },
    }
}

fn start() -> Result<(), Box<dyn Error>> {
    let path = StateDir::default_path().ok_or("Nie można ustalić katalogu stanu aplikacji.")?;
    // Keep simulator ports, journal and history separate from real-printer state.
    #[cfg(feature = "simulator")]
    let path = path.with_file_name("holuto-fiscal-simulator");
    let dir = Arc::new(StateDir::open(path)?);
    let settings = Settings::load(&dir)?.unwrap_or_default();
    #[cfg(feature = "simulator")]
    let simulator = Simulator::start();
    #[cfg(feature = "simulator")]
    let (settings, link): (_, Arc<dyn Link>) = (
        Settings {
            port: simulator.address().to_owned(),
            ..settings
        },
        simulator.link(),
    );
    #[cfg(not(feature = "simulator"))]
    let link: Arc<dyn Link> = Arc::new(SerialLink);
    slint::BackendSelector::new()
        .renderer_name("software".into())
        .select()?;
    application::run(dir, settings, link)?;
    Ok(())
}

fn show_error(message: &str) -> Result<(), slint::PlatformError> {
    let window = ui::ErrorWindow::new()?;
    window.set_message(message.into());
    window.on_dismiss(|| {
        let _event_loop_closed = slint::quit_event_loop();
    });
    window.run()
}
