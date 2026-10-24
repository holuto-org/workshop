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
    unused_qualifications,
    trivial_casts,
    trivial_numeric_casts,
    let_underscore_drop,
    missing_debug_implementations,
    unnameable_types,
    unreachable_pub,
    unused_results,
    clippy::pub_use,
    clippy::absolute_paths,
    clippy::clone_on_ref_ptr,
    clippy::impl_trait_in_params,
    clippy::let_underscore_must_use,
    clippy::as_conversions,
    clippy::float_arithmetic,
    clippy::indexing_slicing,
    clippy::let_underscore_untyped,
    clippy::multiple_inherent_impl,
    clippy::panic,
    clippy::same_name_method,
    clippy::semicolon_outside_block,
    clippy::todo,
    clippy::unwrap_used,
    reason = "Slint 1.18 emits these internal operations; handwritten Rust keeps workspace lints"
)]
mod ui {
    slint::include_modules!();
}

use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

use agent::{settings::Settings, store::StateDir};
use clap::Parser;
#[cfg(feature = "simulator")]
use posnet::simulator::Simulator;
use posnet::{
    printer::Printer,
    session::{Connection, Link, SerialLink},
};
use slint::ComponentHandle;

#[derive(Debug, Parser)]
#[command(version, about = "Holuto Fiscal · Posnet Temo Online")]
struct Arguments {
    /// Odczytaj tożsamość, stan i stawki VAT drukarki, a następnie zakończ.
    #[arg(long)]
    probe: bool,
    /// Port szeregowy drukarki (zastępuje zapisane ustawienie).
    #[arg(long)]
    serial: Option<String>,
    /// Prędkość transmisji (zastępuje zapisane ustawienie).
    #[arg(long)]
    baudrate: Option<u32>,
    /// Katalog stanu aplikacji.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Lokalny symulator TCP. Dostępny wyłącznie w kompilacji debug.
    #[cfg(feature = "simulator")]
    #[arg(long)]
    simulator: bool,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    match start(&arguments) {
        Ok(ready) => {
            if ready {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            if arguments.probe {
                if let Err(write_error) = writeln!(io::stderr().lock(), "{error}") {
                    // There is no second output channel when stderr itself is unavailable.
                    drop(write_error);
                }
            } else if let Err(dialog_error) = show_error(&error.to_string())
                && let Err(write_error) = writeln!(io::stderr().lock(), "{error}\n{dialog_error}")
            {
                drop(write_error);
            }
            ExitCode::FAILURE
        },
    }
}

fn start(arguments: &Arguments) -> Result<bool, Box<dyn Error>> {
    let path = arguments
        .data_dir
        .clone()
        .or_else(StateDir::default_path)
        .ok_or("Nie można ustalić katalogu stanu aplikacji.")?;
    let dir = Arc::new(StateDir::open(path)?);
    let mut settings = Settings::load(&dir)?.unwrap_or_default();
    if let Some(port) = &arguments.serial {
        settings.port.clone_from(port);
    }
    if let Some(baud) = arguments.baudrate {
        settings.baud_rate = baud;
    }
    #[cfg(feature = "simulator")]
    let simulator = arguments.simulator.then(Simulator::start);
    #[cfg(feature = "simulator")]
    let link: Arc<dyn Link> = if let Some(simulator) = &simulator {
        simulator.address().clone_into(&mut settings.port);
        simulator.link()
    } else {
        Arc::new(SerialLink)
    };
    #[cfg(not(feature = "simulator"))]
    let link: Arc<dyn Link> = Arc::new(SerialLink);
    if arguments.probe {
        let status = Printer::new(
            link,
            Connection {
                port: settings.port,
                baud_rate: settings.baud_rate,
            },
        )
        .probe()?;
        let mut stdout = io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &status)?;
        writeln!(stdout)?;
        return Ok(status.ready());
    }
    slint::BackendSelector::new()
        .renderer_name("software".into())
        .select()?;
    application::run(dir, settings, link)?;
    Ok(true)
}

fn show_error(message: &str) -> Result<(), slint::PlatformError> {
    let window = ui::ErrorWindow::new()?;
    window.set_message(message.into());
    window.on_dismiss(|| {
        let _event_loop_closed = slint::quit_event_loop();
    });
    window.run()
}
