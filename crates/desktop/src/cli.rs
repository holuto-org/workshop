//! Command line: a few long options parsed with std, so help and errors stay Polish.

use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    iter::Peekable,
    mem,
    num::ParseIntError,
    path::PathBuf,
};

pub(crate) const VERSION: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));

/// Printed after a usage error.
pub(crate) const HINT: &str = "Pomoc: holuto-fiscal --help";

#[cfg(feature = "simulator")]
const SIMULATOR: &str =
    "\n      --simulator          Lokalny symulator TCP. Dostępny wyłącznie w kompilacji debug.";
#[cfg(not(feature = "simulator"))]
const SIMULATOR: &str = "";

#[derive(Debug)]
pub(crate) enum Command {
    Run(Arguments),
    Help,
    Version,
}

#[derive(Debug, Default)]
pub(crate) struct Arguments {
    /// Read the printer's identity, state and VAT rates, then exit.
    pub(crate) probe: bool,
    /// Overrides the saved serial port.
    pub(crate) serial: Option<String>,
    /// Overrides the saved baud rate.
    pub(crate) baudrate: Option<u32>,
    pub(crate) data_dir: Option<PathBuf>,
    #[cfg(feature = "simulator")]
    pub(crate) simulator: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum UsageError {
    #[error("Nieznany argument „{0}”.")]
    Unknown(String),
    #[error("Opcja {0} wymaga wartości.")]
    MissingValue(&'static str),
    #[error("Opcja {0} nie przyjmuje wartości.")]
    UnexpectedValue(&'static str),
    #[error("Opcję {0} można podać tylko raz.")]
    Repeated(&'static str),
    #[error("Wartość opcji {option} („{value}”) nie jest poprawnym tekstem UTF-8.")]
    NotUnicode { option: &'static str, value: String },
    #[error("Prędkość transmisji „{value}” nie jest liczbą całkowitą z zakresu 0–4294967295.")]
    Baudrate {
        value: String,
        source: ParseIntError,
    },
}

pub(crate) fn write_help(out: &mut dyn Write) -> io::Result<()> {
    writeln!(
        out,
        "Holuto Fiscal · Posnet Temo Online

Użycie: holuto-fiscal [OPCJE]

Opcje:
      --probe              Odczytaj tożsamość, stan i stawki VAT drukarki, a następnie zakończ.
      --serial <PORT>      Port szeregowy drukarki (zastępuje zapisane ustawienie).
      --baudrate <BAUD>    Prędkość transmisji (zastępuje zapisane ustawienie).
      --data-dir <KATALOG> Katalog stanu aplikacji.{SIMULATOR}
  -h, --help               Wyświetl pomoc.
  -V, --version            Wyświetl wersję."
    )
}

/// Parses the arguments after the program name. Help and version answer as soon as they are
/// reached; an option's value is `--name value` or `--name=value`, never another option.
pub(crate) fn parse<I>(args: I) -> Result<Command, UsageError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = Arguments::default();
    let mut args = args.into_iter().peekable();
    while let Some(arg) = args.next() {
        let (name, inline) = split(arg)?;
        match name.as_str() {
            "-h" | "--help" => return flag("--help", inline.as_ref()).map(|()| Command::Help),
            "-V" | "--version" => {
                return flag("--version", inline.as_ref()).map(|()| Command::Version);
            },
            "--probe" => set_flag(&mut arguments.probe, "--probe", inline.as_ref())?,
            #[cfg(feature = "simulator")]
            "--simulator" => set_flag(&mut arguments.simulator, "--simulator", inline.as_ref())?,
            "--serial" => {
                let port = text("--serial", value("--serial", inline, &mut args)?)?;
                set(&mut arguments.serial, "--serial", port)?;
            },
            "--baudrate" => {
                let baud = text("--baudrate", value("--baudrate", inline, &mut args)?)?;
                let baud = baud.parse().map_err(|source| UsageError::Baudrate {
                    value: baud,
                    source,
                })?;
                set(&mut arguments.baudrate, "--baudrate", baud)?;
            },
            "--data-dir" => {
                let dir = value("--data-dir", inline, &mut args)?;
                set(&mut arguments.data_dir, "--data-dir", dir.into())?;
            },
            _ => return Err(UsageError::Unknown(name)),
        }
    }
    Ok(Command::Run(arguments))
}

/// Splits `--name=value` at the first `=`. The value keeps what is not UTF-8: a path may.
fn split(arg: OsString) -> Result<(String, Option<OsString>), UsageError> {
    match arg.into_string() {
        Ok(mut name) => {
            let value = name
                .starts_with("--")
                .then(|| name.find('='))
                .flatten()
                .map(|at| {
                    let value = name.split_off(at + 1);
                    name.truncate(at);
                    OsString::from(value)
                });
            Ok((name, value))
        },
        Err(arg) => split_lossless(&arg)
            .filter(|(name, _)| name.starts_with("--"))
            .ok_or_else(|| UsageError::Unknown(arg.display().to_string())),
    }
}

#[cfg(unix)]
fn split_lossless(arg: &OsStr) -> Option<(String, Option<OsString>)> {
    use std::os::unix::ffi::OsStrExt;

    let mut parts = arg.as_bytes().splitn(2, |&byte| byte == b'=');
    let name = str::from_utf8(parts.next()?).ok()?.to_owned();
    Some((name, Some(OsStr::from_bytes(parts.next()?).to_owned())))
}

#[cfg(windows)]
fn split_lossless(arg: &OsStr) -> Option<(String, Option<OsString>)> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let wide: Vec<u16> = arg.encode_wide().collect();
    let mut parts = wide.splitn(2, |&unit| unit == u16::from(b'='));
    let name = String::from_utf16(parts.next()?).ok()?;
    Some((name, Some(OsString::from_wide(parts.next()?))))
}

fn value<I>(
    option: &'static str,
    inline: Option<OsString>,
    args: &mut Peekable<I>,
) -> Result<OsString, UsageError>
where
    I: Iterator<Item = OsString>,
{
    inline
        .or_else(|| args.next_if(|next| !is_option(next)))
        .filter(|value| !value.is_empty())
        .ok_or(UsageError::MissingValue(option))
}

/// `-` alone is a value (conventionally stdin); anything else starting with `-` is an option.
fn is_option(arg: &OsStr) -> bool {
    arg.as_encoded_bytes().starts_with(b"-") && arg != "-"
}

fn text(option: &'static str, value: OsString) -> Result<String, UsageError> {
    value.into_string().map_err(|value| UsageError::NotUnicode {
        option,
        value: value.display().to_string(),
    })
}

const fn flag(option: &'static str, inline: Option<&OsString>) -> Result<(), UsageError> {
    match inline {
        Some(_) => Err(UsageError::UnexpectedValue(option)),
        None => Ok(()),
    }
}

fn set_flag(
    slot: &mut bool,
    option: &'static str,
    inline: Option<&OsString>,
) -> Result<(), UsageError> {
    flag(option, inline)?;
    if mem::replace(slot, true) {
        return Err(UsageError::Repeated(option));
    }
    Ok(())
}

fn set<T>(slot: &mut Option<T>, option: &'static str, value: T) -> Result<(), UsageError> {
    if slot.is_some() {
        return Err(UsageError::Repeated(option));
    }
    *slot = Some(value);
    Ok(())
}

#[cfg(test)]
mod tests;
