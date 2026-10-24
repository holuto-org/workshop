use std::{
    error::Error,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use agent::{settings::Settings, store::StateDir};
use tempfile::TempDir;

use super::{Outcome, Request, Writer};

const WAIT: Duration = Duration::from_secs(10);

fn settings() -> Settings {
    Settings {
        api_url: "http://127.0.0.1:43123/workshop".into(),
        token: "preference-test-token".into(),
        port: "operator-selected-port".into(),
        baud_rate: 57_600,
        telemetry_enabled: true,
        start_on_launch: false,
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn queued_save_port_and_opt_out_preserve_every_connection_field() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let observed_dir = Arc::clone(&dir);
    let (completed, completions) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        let _sent = completed.send((completion, Settings::load(&observed_dir)));
    })?;
    let supplied = settings();

    writer.submit(Request::Save {
        settings: supplied.clone(),
        start: false,
    })?;
    writer.submit(Request::Port("detected-printer-port".into()))?;
    writer.submit(Request::Telemetry(false))?;

    let (completion, stored) = completions.recv_timeout(WAIT)?;
    assert_eq!(completion, Ok(Outcome::Saved), "save is acknowledged first");
    assert_eq!(
        stored?,
        Some(supplied.clone()),
        "save persists every supplied field"
    );

    let detected = Settings {
        port: "detected-printer-port".into(),
        ..supplied
    };
    let (completion, stored) = completions.recv_timeout(WAIT)?;
    assert_eq!(completion, Ok(Outcome::Port), "port is acknowledged second");
    assert_eq!(
        stored?,
        Some(detected.clone()),
        "detection changes only the port"
    );

    let opted_out = Settings {
        telemetry_enabled: false,
        ..detected
    };
    let (completion, stored) = completions.recv_timeout(WAIT)?;
    assert_eq!(
        completion,
        Ok(Outcome::Telemetry),
        "opt-out is acknowledged last"
    );
    assert_eq!(
        stored?,
        Some(opted_out.clone()),
        "opt-out retains the detected connection"
    );
    assert_eq!(
        Settings::load(&dir)?,
        Some(opted_out),
        "final preferences retain all queued edits"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn start_continuation_observes_its_exact_configuration_already_persisted()
-> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let observed_dir = Arc::clone(&dir);
    let (completed, completions) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        let _sent = completed.send((completion, Settings::load(&observed_dir)));
    })?;
    let supplied = Settings {
        telemetry_enabled: false,
        ..settings()
    };

    writer.submit(Request::Save {
        settings: supplied.clone(),
        start: true,
    })?;

    let (completion, stored_at_continuation) = completions.recv_timeout(WAIT)?;
    assert_eq!(
        completion,
        Ok(Outcome::Start(supplied.clone())),
        "start receives the acknowledged configuration"
    );
    assert_eq!(
        stored_at_continuation?,
        Some(supplied),
        "start cannot precede durable persistence of its configuration"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn held_completion_does_not_block_submitting_the_next_preferences() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let (completed, completions) = mpsc::channel();
    let (reached, held) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        if matches!(&completion, Ok(Outcome::Start(_))) {
            let _announced = reached.send(());
            let _released = released.recv();
        }
        let _sent = completed.send(completion);
    })?;
    let supplied = settings();
    writer.submit(Request::Save {
        settings: supplied.clone(),
        start: true,
    })?;
    held.recv_timeout(WAIT)?;

    let (submitted, submission) = mpsc::channel();
    let (submission_result, release_result, joined) = thread::scope(|scope| {
        let sender = scope.spawn(|| {
            let result = writer
                .submit(Request::Port("detected-while-completion-held".into()))
                .and_then(|()| writer.submit(Request::Telemetry(false)));
            let _sent = submitted.send(result);
        });
        let result = submission.recv_timeout(WAIT);
        // Release before propagating a timeout so a blocked sender can finish and be joined.
        let release_result = release.send(());
        (result, release_result, sender.join())
    });
    joined.map_err(|_panic| "preference submitter panicked")?;
    release_result?;
    submission_result??;

    assert_eq!(
        completions.recv_timeout(WAIT)?,
        Ok(Outcome::Start(supplied.clone())),
        "the held save remains the first completion"
    );
    assert_eq!(
        completions.recv_timeout(WAIT)?,
        Ok(Outcome::Port),
        "queued detection follows the save"
    );
    assert_eq!(
        completions.recv_timeout(WAIT)?,
        Ok(Outcome::Telemetry),
        "queued opt-out follows detection"
    );
    assert_eq!(
        Settings::load(&dir)?,
        Some(Settings {
            port: "detected-while-completion-held".into(),
            telemetry_enabled: false,
            ..supplied
        }),
        "pending completion delivery cannot lose or reorder later preferences"
    );
    Ok(())
}
