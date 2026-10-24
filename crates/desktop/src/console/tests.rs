use std::{
    env,
    error::Error,
    io::{self, Write as _},
    process::Command,
};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove pipes; setup propagates I/O"
)]
fn cli_attachment_preserves_captured_subprocess_output() -> TestResult {
    if env::var_os("WORKSHOP_CONSOLE_CHILD").is_some() {
        super::prepare(true);
        writeln!(io::stdout().lock(), "workshop-stdout")?;
        writeln!(io::stderr().lock(), "workshop-stderr")?;
        return Ok(());
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "Child isolates native console handles"
    )]
    let output = Command::new(env::current_exe()?)
        .args([
            "--exact",
            "console::tests::cli_attachment_preserves_captured_subprocess_output",
            "--nocapture",
        ])
        .env("WORKSHOP_CONSOLE_CHILD", "1")
        .output()?;
    assert!(output.status.success(), "child process: {output:?}");
    assert!(String::from_utf8(output.stdout)?.contains("workshop-stdout"));
    assert_eq!(String::from_utf8(output.stderr)?, "workshop-stderr\n");
    Ok(())
}
