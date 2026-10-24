#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(all(feature = "simulator", not(debug_assertions)))]
compile_error!("The simulator feature is restricted to local debug builds.");

mod application;
mod autostart;
mod cli;
mod console;
mod model;
mod telemetry;
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
    env,
    error::Error,
    io::{self, Write},
    process::ExitCode,
    sync::Arc,
};

use agent::{settings::Settings, store::StateDir};
use fiscal::link::Link;
#[cfg(not(feature = "simulator"))]
use fiscal::link::SerialLink;
#[cfg(feature = "simulator")]
use posnet::simulator::Simulator;
use slint::ComponentHandle;

fn main() -> ExitCode {
    // Native reporters bypass CLI dispatch, the instance lock and fiscal storage.
    if telemetry::is_crash_reporter_process() {
        let _telemetry = telemetry::init();
        return ExitCode::FAILURE;
    }
    console::prepare(env::args_os().nth(1).is_some());
    if let Some(code) = cli::run(env::args_os().skip(1)) {
        return code;
    }
    let _telemetry = telemetry::init();
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
    let dir = Arc::new(StateDir::open_default(cfg!(feature = "simulator"))?);
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
    slint::set_xdg_app_id("workshop")?;
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
