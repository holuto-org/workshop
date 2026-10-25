//! Terminal-only configuration; never opens a printer or starts fiscal processing.

use std::{
    error::Error as StdError,
    ffi::OsString,
    fmt,
    io::{self, Write as _},
    path::Path,
    process::ExitCode,
    sync::{Arc, mpsc},
};

use agent::{
    Error,
    api::ApiError,
    settings::Settings,
    status::Snapshot,
    store::StateDir,
    worker::{Event, PairingFailure, Worker},
};
use fiscal::link;
use serde_json::{Value, json};

use crate::autostart;

const HELP: &str = "Holuto Workshop\n\n\
Użycie: workshop [polecenie] [--json]\n\
Bez argumentów otwiera aplikację.\n\n\
  pair <KOD>                         Sparuj drukarkę z Holuto\n\
  config show                        Pokaż ustawienia bez sekretów\n\
  config set [--api-url URL] [--port auto|PORT] [--speed LICZBA] [--diagnostics on|off]\n\
  ports                              Wypisz nazwy portów (bez otwierania)\n\
  check                              Sprawdź konfigurację i blokadę (bez połączeń)\n\
  status                             Stan aplikacji, połączenia i blokady fiskalnej\n\
  autostart on|off                   Start aplikacji i przetwarzania po zalogowaniu\n\
  --help                             Pomoc\n\
  --version                          Wersja\n\n\
Kody wyjścia: 0 sukces, 1 błąd lokalny, 2 argumenty, 3 konfiguracja/kod,\n\
4 aplikacja działa, 5 komunikacja Holuto, 6 brak autoryzacji, 7 limit parowania.\n";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Version,
    ConfigShow,
    ConfigSet(Changes),
    Pair(String),
    Ports,
    Check,
    Status,
    Autostart(bool),
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Changes {
    api_url: Option<String>,
    port: Option<String>,
    speed: Option<u32>,
    diagnostics: Option<bool>,
}

#[derive(Debug)]
struct Parsed {
    command: Command,
    json: bool,
}

#[derive(Debug)]
struct Failure {
    exit: u8,
    code: &'static str,
    message: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl StdError for Failure {}

impl From<Error> for Failure {
    fn from(value: Error) -> Self {
        let (exit, code) = match &value {
            Error::Invalid(_) => (3, "configuration"),
            Error::Blocked(_) => (4, "app_running"),
            Error::Api(ApiError::Unauthorized) => (6, "unauthorized"),
            Error::Api(ApiError::Status { status: 429, .. }) => (7, "rate_limited"),
            Error::Api(_) => (5, "holuto"),
            Error::Store { .. } | Error::Printer(_) => (1, "local"),
        };
        Self {
            exit,
            code,
            message: value.to_string(),
        }
    }
}

#[derive(Debug)]
struct Response {
    value: Value,
    human: String,
    exit: u8,
}

fn arguments() -> Failure {
    Failure {
        exit: 2,
        code: "arguments",
        message: "Nieprawidłowe argumenty. Użyj workshop --help.".into(),
    }
}

fn switch(value: &str) -> Result<bool, Failure> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(arguments()),
    }
}

fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Parsed, Failure> {
    let mut json = false;
    let mut words = Vec::new();
    for word in args {
        let word = word.into_string().map_err(|_word| arguments())?;
        if word == "--json" {
            if json {
                return Err(arguments());
            }
            json = true;
        } else {
            words.push(word);
        }
    }
    let command = match words.as_slice() {
        [help] if help == "--help" || help == "-h" => Command::Help,
        [version] if version == "--version" || version == "-V" => Command::Version,
        [config, show] if config == "config" && show == "show" => Command::ConfigShow,
        [pair, code] if pair == "pair" => Command::Pair(code.clone()),
        [ports] if ports == "ports" => Command::Ports,
        [check] if check == "check" => Command::Check,
        [status] if status == "status" => Command::Status,
        [autostart, value] if autostart == "autostart" => Command::Autostart(switch(value)?),
        [config, set, rest @ ..] if config == "config" && set == "set" && !rest.is_empty() => {
            let mut changes = Changes::default();
            for pair in rest.chunks(2) {
                let [flag, value] = pair else {
                    return Err(arguments());
                };
                match flag.as_str() {
                    "--api-url" if changes.api_url.is_none() => {
                        changes.api_url = Some(value.clone());
                    },
                    "--port" if changes.port.is_none() => {
                        changes.port = Some(if value == "auto" {
                            String::new()
                        } else {
                            value.clone()
                        });
                    },
                    "--speed" if changes.speed.is_none() => {
                        changes.speed = Some(value.parse().map_err(|_error| arguments())?);
                    },
                    "--diagnostics" if changes.diagnostics.is_none() => {
                        changes.diagnostics = Some(switch(value)?);
                    },
                    _ => return Err(arguments()),
                }
            }
            Command::ConfigSet(changes)
        },
        _ => return Err(arguments()),
    };
    Ok(Parsed { command, json })
}

pub(crate) fn run<I: IntoIterator<Item = OsString>>(args: I) -> Option<ExitCode> {
    let args: Vec<_> = args.into_iter().collect();
    if args.is_empty() {
        return None;
    }
    let mut json = args.iter().any(|word| word == "--json");
    let result = parse(args).and_then(|parsed| {
        json = parsed.json;
        match parsed.command {
            command @ (Command::Help | Command::Version | Command::Ports) => {
                execute(command, Path::new(""))
            },
            command @ (Command::ConfigShow
            | Command::ConfigSet(_)
            | Command::Pair(_)
            | Command::Check
            | Command::Status
            | Command::Autostart(_)) => {
                let path =
                    StateDir::default_path_for(cfg!(feature = "simulator")).ok_or_else(|| {
                        Failure {
                            exit: 1,
                            code: "local",
                            message: "Nie można ustalić katalogu stanu aplikacji.".into(),
                        }
                    })?;
                execute(command, &path)
            },
        }
    });
    let Response { value, human, exit } = result.unwrap_or_else(|error| Response {
        value: json!({"ok":false,"error":{"code":error.code,"message":error.message}}),
        human: error.message,
        exit: error.exit,
    });
    let output = if json { value.to_string() } else { human };
    let written = if !json && exit != 0 {
        writeln!(io::stderr().lock(), "{output}")
    } else {
        writeln!(io::stdout().lock(), "{output}")
    };
    Some(ExitCode::from(if written.is_ok() { exit } else { 1 }))
}

fn execute(command: Command, path: &Path) -> Result<Response, Failure> {
    match command {
        Command::Help => Ok(Response {
            value: json!({"ok":true,"help":HELP}),
            human: HELP.into(),
            exit: 0,
        }),
        Command::Version => Ok(Response {
            value: json!({"ok":true,"name":"workshop","version":env!("CARGO_PKG_VERSION")}),
            human: format!("Holuto Workshop {}", env!("CARGO_PKG_VERSION")),
            exit: 0,
        }),
        Command::ConfigShow => {
            let settings = Settings::load_at(path)?.unwrap_or_default();
            settings.validate_configuration()?;
            let value = json!({
                "ok":true,
                "config":{
                    "apiUrl":settings.api_url,
                    "paired":!settings.token.as_str().is_empty(),
                    "port":if settings.port.is_empty() { "auto" } else { &settings.port },
                    "baudRate":settings.baud_rate,
                    "vendor":settings.vendor,
                    "telemetryEnabled":settings.telemetry_enabled,
                    "startOnLaunch":settings.start_on_launch
                }
            });
            let human = serde_json::to_string_pretty(&value).map_err(|_error| arguments())?;
            Ok(Response {
                value,
                human,
                exit: 0,
            })
        },
        Command::Check => {
            let settings = Settings::load_at(path)?.unwrap_or_default();
            let app_running = StateDir::is_locked(path)?;
            let errors: Vec<_> = settings
                .validate()
                .err()
                .map(|error| error.to_string())
                .into_iter()
                .collect();
            let configured = errors.is_empty();
            Ok(Response {
                value: json!({
                    "ok":configured, "configured":configured,
                    "appRunning":app_running, "errors":errors
                }),
                human: if configured {
                    "Konfiguracja jest prawidłowa. Nie nawiązano połączenia z drukarką ani Holuto."
                        .into()
                } else {
                    errors.join("\n")
                },
                exit: if configured { 0 } else { 3 },
            })
        },
        Command::Status => status(path),
        Command::Ports => Ok(ports_response(&link::serial_ports())),
        Command::ConfigSet(changes) => config_set(path, changes),
        Command::Autostart(enabled) => {
            let dir = StateDir::open(path.to_owned())?;
            let settings = Settings::load(&dir)?.unwrap_or_default();
            autostart::configure(&dir, settings, enabled)?;
            Ok(Response {
                value: json!({"ok":true,"autostart":enabled,"startOnLaunch":enabled}),
                human: if enabled {
                    "Włączono autostart aplikacji i przetwarzania.".into()
                } else {
                    "Wyłączono autostart aplikacji i przetwarzania.".into()
                },
                exit: 0,
            })
        },
        Command::Pair(code) => pair(path, code),
    }
}

fn ports_response(ports: &[link::Candidate]) -> Response {
    let mut human = String::new();
    for (index, candidate) in ports.iter().enumerate() {
        if index > 0 {
            human.push('\n');
        }
        human.push_str(&candidate.port);
    }
    Response {
        value: json!({"ok":true,"ports":ports}),
        human,
        exit: 0,
    }
}

fn status(path: &Path) -> Result<Response, Failure> {
    let stopped = || Response {
        value: json!({"ok":true,"appRunning":false}),
        human: "Aplikacja jest zatrzymana.".into(),
        exit: 0,
    };
    if !StateDir::is_locked(path)? {
        return Ok(stopped());
    }
    let snapshot = Snapshot::load_at(path);
    // An app can exit during the read; a leftover snapshot is never live state.
    if !StateDir::is_locked(path)? {
        return Ok(stopped());
    }
    let snapshot = snapshot?.ok_or_else(|| Failure {
        exit: 1,
        code: "status_unavailable",
        message: "Aplikacja nie udostępniła jeszcze stanu. Spróbuj ponownie.".into(),
    })?;
    let human = format!(
        "Aplikacja działa. Przetwarzanie: {}. Połączenie z drukarką: {}. \
         Blokada fiskalna: {}. Stan z {}.",
        if snapshot.processing {
            "włączone"
        } else {
            "wyłączone"
        },
        if snapshot.connected {
            "aktywne"
        } else {
            "nieaktywne"
        },
        if snapshot.blocked { "tak" } else { "nie" },
        snapshot.updated_at
    );
    Ok(Response {
        value: json!({"ok":true,"appRunning":true,"state":snapshot}),
        human,
        exit: 0,
    })
}

fn config_set(path: &Path, changes: Changes) -> Result<Response, Failure> {
    let dir = StateDir::open(path.to_owned())?;
    let mut settings = Settings::load(&dir)?.unwrap_or_default();
    if let Some(api_url) = changes.api_url {
        settings.set_api_url(&api_url);
    }
    if let Some(port) = changes.port {
        settings.port = port;
    }
    if let Some(speed) = changes.speed {
        settings.baud_rate = speed;
    }
    if let Some(enabled) = changes.diagnostics {
        settings.telemetry_enabled = enabled;
    }
    settings.save(&dir)?;
    Ok(Response {
        value: json!({"ok":true}),
        human: "Zapisano ustawienia.".into(),
        exit: 0,
    })
}

fn pair(path: &Path, code: String) -> Result<Response, Failure> {
    let dir = Arc::new(StateDir::open(path.to_owned())?);
    let settings = Settings::load(&dir)?.unwrap_or_default();
    let (sender, receiver) = mpsc::channel();
    let worker = Worker::pair(dir, settings, code, move |event| {
        let _sent = sender.send(event);
    })?;
    let mut result = Err(Failure {
        exit: 1,
        code: "local",
        message: "Parowanie nie zostało zakończone.".into(),
    });
    while let Ok(event) = receiver.recv() {
        match event {
            Event::Paired { printer_id } => {
                result = Ok(Response {
                    value: json!({"ok":true,"printerId":printer_id}),
                    human: "Sparowano drukarkę z Holuto.".into(),
                    exit: 0,
                });
            },
            Event::PairingFailed { message, failure } => {
                let (exit, code) = match failure {
                    PairingFailure::Invalid => (3, "pairing_invalid"),
                    PairingFailure::Communication => (5, "holuto"),
                    PairingFailure::Unauthorized => (6, "unauthorized"),
                    PairingFailure::Throttled => (7, "rate_limited"),
                    PairingFailure::Storage => (1, "local"),
                };
                result = Err(Failure {
                    exit,
                    code,
                    message,
                });
            },
            Event::Stopped => {
                worker.join();
                return result;
            },
            Event::Status { .. }
            | Event::PortDetected { .. }
            | Event::Connected { .. }
            | Event::Documents { .. }
            | Event::Device { .. }
            | Event::HistoryChanged(_)
            | Event::CommandCompleted => {},
        }
    }
    worker.join();
    result
}

#[cfg(test)]
mod tests;
