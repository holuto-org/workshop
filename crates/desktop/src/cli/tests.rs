#![expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating parse failures"
)]

use std::{error::Error, path::Path};

use super::*;

fn parse_strs(args: &[&str]) -> Result<Command, UsageError> {
    parse(args.iter().map(OsString::from))
}

fn run(args: &[&str]) -> Result<Arguments, Box<dyn Error>> {
    match parse_strs(args)? {
        Command::Run(arguments) => Ok(arguments),
        other @ (Command::Help | Command::Version) => Err(format!("{other:?}").into()),
    }
}

#[test]
fn options_take_separate_or_inline_values_split_at_the_first_equals() -> Result<(), Box<dyn Error>>
{
    let separate = run(&[
        "--serial",
        "/dev/ttyACM0",
        "--baudrate",
        "115200",
        "--data-dir",
        "/tmp/a=b",
        "--probe",
    ])?;
    assert_eq!(
        separate.serial.as_deref(),
        Some("/dev/ttyACM0"),
        "{separate:?}"
    );
    assert_eq!(separate.baudrate, Some(115_200), "{separate:?}");
    assert_eq!(
        separate.data_dir.as_deref(),
        Some(Path::new("/tmp/a=b")),
        "{separate:?}"
    );
    assert!(separate.probe, "{separate:?}");

    let inline = run(&["--serial=COM3", "--baudrate=9600", "--data-dir=C:\\a=b"])?;
    assert_eq!(inline.serial.as_deref(), Some("COM3"), "{inline:?}");
    assert_eq!(inline.baudrate, Some(9600), "{inline:?}");
    assert_eq!(
        inline.data_dir.as_deref(),
        Some(Path::new("C:\\a=b")),
        "{inline:?}"
    );
    assert!(!inline.probe, "{inline:?}");
    Ok(())
}

#[test]
fn help_and_version_answer_only_when_reached_before_an_error() {
    for (arg, help) in [
        ("-h", true),
        ("--help", true),
        ("-V", false),
        ("--version", false),
    ] {
        let parsed = parse_strs(&["--probe", arg, "--bogus"]);
        assert!(
            if help {
                matches!(parsed, Ok(Command::Help))
            } else {
                matches!(parsed, Ok(Command::Version))
            },
            "{arg}: {parsed:?}"
        );
    }
    let parsed = parse_strs(&["--bogus", "--help"]);
    assert!(matches!(parsed, Err(UsageError::Unknown(_))), "{parsed:?}");
}

#[test]
fn short_clusters_answer_with_their_first_flag() {
    for (arg, help) in [("-hV", true), ("-hx", true), ("-h=1", true), ("-Vh", false)] {
        let parsed = parse_strs(&[arg]);
        assert!(
            if help {
                matches!(parsed, Ok(Command::Help))
            } else {
                matches!(parsed, Ok(Command::Version))
            },
            "{arg}: {parsed:?}"
        );
    }
    let parsed = parse_strs(&["-xh"]);
    assert!(matches!(parsed, Err(UsageError::Unknown(_))), "{parsed:?}");
}

#[test]
fn double_dash_ends_the_options_and_nothing_may_follow() -> Result<(), Box<dyn Error>> {
    assert!(run(&["--probe", "--"])?.probe, "options before `--` still apply");
    let parsed = parse_strs(&["--", "--probe"]);
    assert!(matches!(parsed, Err(UsageError::Unknown(_))), "{parsed:?}");
    Ok(())
}

#[test]
fn an_empty_value_is_a_port_but_not_a_directory_or_baud_rate() -> Result<(), Box<dyn Error>> {
    for args in [&["--serial="][..], &["--serial", ""]] {
        let arguments = run(args)?;
        assert_eq!(arguments.serial.as_deref(), Some(""), "{args:?}: {arguments:?}");
    }
    for args in [&["--data-dir="][..], &["--data-dir", ""]] {
        let parsed = parse_strs(args);
        assert!(
            matches!(parsed, Err(UsageError::MissingValue("--data-dir"))),
            "{args:?}: {parsed:?}"
        );
    }
    let parsed = parse_strs(&["--baudrate="]);
    assert!(matches!(parsed, Err(UsageError::Baudrate { .. })), "{parsed:?}");
    Ok(())
}

#[test]
fn a_value_is_never_taken_from_the_next_option() {
    // Otherwise `--serial --probe` would open the app on a port named "--probe".
    for (args, option) in [
        (&["--serial", "--probe"][..], "--serial"),
        (&["--data-dir", "--probe"], "--data-dir"),
        (&["--baudrate", "-1"], "--baudrate"),
        (&["--probe", "--serial"], "--serial"),
        (&["--data-dir", "--"], "--data-dir"),
    ] {
        let parsed = parse_strs(args);
        assert!(
            matches!(parsed, Err(UsageError::MissingValue(missing)) if missing == option),
            "{args:?}: {parsed:?}"
        );
    }
}

#[test]
fn options_given_twice_are_rejected() {
    for args in [
        &["--probe", "--probe"][..],
        &["--serial=COM1", "--serial", "COM2"],
    ] {
        let parsed = parse_strs(args);
        assert!(
            matches!(parsed, Err(UsageError::Repeated(_))),
            "{args:?}: {parsed:?}"
        );
    }
}

#[test]
fn flags_reject_values() {
    for arg in ["--probe=1", "--help=1", "--version=1"] {
        let parsed = parse_strs(&[arg]);
        assert!(
            matches!(parsed, Err(UsageError::UnexpectedValue(_))),
            "{arg}: {parsed:?}"
        );
    }
}

#[test]
fn unknown_abbreviated_and_positional_arguments_are_rejected() {
    for arg in ["--prob", "-p", "-xh", "-", "COM3", "--bogus=1", "--=1"] {
        let parsed = parse_strs(&[arg]);
        assert!(
            matches!(parsed, Err(UsageError::Unknown(_))),
            "{arg}: {parsed:?}"
        );
    }
}

#[test]
fn baud_rate_must_be_an_unsigned_32_bit_integer() {
    for value in ["abc", "9600.0", "4294967296"] {
        let parsed = parse_strs(&["--baudrate", value]);
        assert!(
            matches!(parsed, Err(UsageError::Baudrate { .. })),
            "{value}: {parsed:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn paths_that_are_not_unicode_survive_both_forms() -> Result<(), Box<dyn Error>> {
    use std::os::unix::ffi::OsStringExt;

    let path = OsString::from_vec(b"/tmp/\xFF".to_vec());
    let inline = OsString::from_vec(b"--data-dir=/tmp/\xFF".to_vec());
    let serial = OsString::from_vec(b"--serial=\xFF".to_vec());
    assert_not_unicode_paths(&path, inline, serial)
}

#[cfg(windows)]
#[test]
fn paths_that_are_not_unicode_survive_both_forms() -> Result<(), Box<dyn Error>> {
    use std::os::windows::ffi::OsStringExt;

    // A lone surrogate: valid in a Windows path, not in UTF-8.
    let wide = |text: &str| text.encode_utf16().chain([0xD800]).collect::<Vec<u16>>();
    let path = OsString::from_wide(&wide("C:\\tmp\\"));
    let inline = OsString::from_wide(&wide("--data-dir=C:\\tmp\\"));
    let serial = OsString::from_wide(&wide("--serial="));
    assert_not_unicode_paths(&path, inline, serial)
}

fn assert_not_unicode_paths(
    path: &OsStr,
    inline: OsString,
    serial: OsString,
) -> Result<(), Box<dyn Error>> {
    for args in [
        vec![OsString::from("--data-dir"), path.to_owned()],
        vec![inline],
    ] {
        let Command::Run(arguments) = parse(args)? else {
            return Err("expected a run".into());
        };
        assert_eq!(
            arguments.data_dir.as_deref(),
            Some(Path::new(path)),
            "{arguments:?}"
        );
    }
    let parsed = parse([serial]);
    assert!(
        matches!(
            parsed,
            Err(UsageError::NotUnicode {
                option: "--serial",
                ..
            })
        ),
        "{parsed:?}"
    );
    Ok(())
}
