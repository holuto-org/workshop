#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(all(feature = "simulator", not(debug_assertions)))]
compile_error!("The simulator feature is restricted to local debug builds.");

mod application;
mod cli;
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
    env,
    error::Error,
    io::{self, Write},
    process::ExitCode,
    sync::Arc,
};

use agent::{settings::Settings, store::StateDir};
use cli::{Arguments, Command};
#[cfg(feature = "simulator")]
use posnet::simulator::Simulator;
use posnet::{
    printer::Printer,
    session::{Connection, Link, SerialLink},
};
use slint::ComponentHandle;

fn main() -> ExitCode {
    let arguments = match cli::parse(env::args_os().skip(1)) {
        Ok(Command::Run(arguments)) => arguments,
        Ok(Command::Help) => {
            return cli::write_help(&mut io::stdout().lock())
                .map_or(ExitCode::FAILURE, |()| ExitCode::SUCCESS);
        },
        Ok(Command::Version) => {
            return writeln!(io::stdout().lock(), "{}", cli::VERSION)
                .map_or(ExitCode::FAILURE, |()| ExitCode::SUCCESS);
        },
        Err(error) => {
            if let Err(write_error) = writeln!(io::stderr().lock(), "{error}\n{}", cli::HINT) {
                // There is no second output channel when stderr itself is unavailable.
                drop(write_error);
            }
            // The conventional usage-error status.
            return ExitCode::from(2);
        },
    };
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
