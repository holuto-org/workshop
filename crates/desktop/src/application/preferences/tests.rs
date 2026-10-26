use std::{
    error::Error,
    fs,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use agent::{settings::Settings, status::Snapshot, store::StateDir};
use fiscal::device::Vendor;
use tempfile::TempDir;

use super::{Outcome, Request, Writer};

const WAIT: Duration = Duration::from_secs(10);

fn settings() -> Settings {
    Settings {
        api_url: "http://127.0.0.1:43123/workshop".into(),
        token: "preference-test-token".into(),
        port: "operator-selected-port".into(),
        baud_rate: 57_600,
        vendor: None,
        telemetry_enabled: true,
        start_on_launch: false,
    }
}

fn paired(temporary: &TempDir) -> Result<Arc<StateDir>, Box<dyn Error>> {
    let dir = StateDir::open(temporary.path().to_owned())?;
    let stored = settings();
    Settings {
        api_url: stored.api_url,
        token: stored.token,
        ..Settings::default()
    }
    .save(&dir)?;
    Ok(Arc::new(dir))
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn start_without_a_stored_pairing_is_refused_before_writing() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let unpaired = Settings {
        api_url: settings().api_url,
        ..Settings::default()
    };
    unpaired.save(&dir)?;
    let (completed, completions) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        let _sent = completed.send(completion);
    })?;
    // The window still holds a credential the store no longer has.
    writer.submit(Request::Save {
        settings: settings(),
        start: true,
    })?;
    assert_eq!(
        completions.recv_timeout(WAIT)?,
        Err("Sparuj drukarkę kodem z Holuto w Ustawieniach.".to_owned())
    );
    assert_eq!(
        Settings::load(&dir)?,
        Some(unpaired),
        "a refused start writes nothing"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn queued_save_port_and_opt_out_preserve_every_connection_field() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = paired(&temporary)?;
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
    writer.submit(Request::Port {
        port: "detected-printer-port".into(),
        vendor: Vendor::Novitus,
    })?;
    writer.submit(Request::Telemetry(false))?;

    let (completion, stored) = completions.recv_timeout(WAIT)?;
    assert_eq!(
        completion,
        Ok(Outcome::Saved(supplied.clone())),
        "save is acknowledged first"
    );
    assert_eq!(
        stored?,
        Some(supplied.clone()),
        "save persists every supplied field"
    );

    let detected = Settings {
        port: "detected-printer-port".into(),
        vendor: Some(Vendor::Novitus),
        ..supplied
    };
    let (completion, stored) = completions.recv_timeout(WAIT)?;
    assert_eq!(completion, Ok(Outcome::Port), "port is acknowledged second");
    assert_eq!(
        stored?,
        Some(detected.clone()),
        "detection persists the port and vendor together"
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
    let dir = paired(&temporary)?;
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
    let dir = paired(&temporary)?;
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
                .submit(Request::Port {
                    port: "detected-while-completion-held".into(),
                    vendor: Vendor::Elzab,
                })
                .and_then(|()| writer.submit(Request::Telemetry(false)));
            let _sent = submitted.send(result);
        });
        let result = submission.recv_timeout(WAIT);
        // Release before propagating failure so the sender can finish and join.
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
            vendor: Some(Vendor::Elzab),
            telemetry_enabled: false,
            ..supplied
        }),
        "pending completion delivery cannot lose or reorder later preferences"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn status_and_preferences_share_one_durable_fifo() -> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = paired(&temporary)?;
    let before_save = Settings::load(&dir)?;
    let observed_dir = Arc::clone(&dir);
    let (completed, completions) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        let _sent = completed.send((
            completion,
            Snapshot::load_at(observed_dir.path()),
            Settings::load(&observed_dir),
        ));
    })?;
    let connected = Snapshot {
        processing: true,
        connected: true,
        vendor: Some(Vendor::Posnet),
        unique_number: Some("DEM0000000001".into()),
        printer_id: Some("printer-42".into()),
        blocked: false,
        report_unresolved: false,
        orphaned_receipt: false,
        updated_at: "2025-10-24T10:00:00Z".parse()?,
    };
    let stopped = Snapshot {
        processing: false,
        connected: false,
        blocked: true,
        report_unresolved: true,
        updated_at: "2025-10-24T10:00:01Z".parse()?,
        ..connected.clone()
    };
    writer.submit(Request::Status(connected.clone()))?;
    writer.submit(Request::Save {
        settings: settings(),
        start: false,
    })?;
    writer.submit(Request::Status(stopped.clone()))?;

    let (outcome, status, stored_settings) = completions.recv_timeout(WAIT)?;
    assert_eq!(outcome, Ok(Outcome::Status(connected.clone())));
    assert_eq!(status?, Some(connected.clone()));
    assert_eq!(
        stored_settings?, before_save,
        "later preferences cannot overtake status"
    );
    let (outcome, status, stored_settings) = completions.recv_timeout(WAIT)?;
    assert_eq!(outcome, Ok(Outcome::Saved(settings())));
    assert_eq!(status?, Some(connected));
    assert_eq!(stored_settings?, Some(settings()));
    let (outcome, status, stored_settings) = completions.recv_timeout(WAIT)?;
    assert_eq!(outcome, Ok(Outcome::Status(stopped.clone())));
    assert_eq!(status?, Some(stopped));
    assert_eq!(stored_settings?, Some(settings()));
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn failed_status_write_is_not_acknowledged_as_saved_or_blocking_later_preferences()
-> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    fs::create_dir(temporary.path().join("status.json"))?;
    let (completed, completions) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        let _sent = completed.send(completion);
    })?;
    writer.submit(Request::Status(Snapshot {
        processing: false,
        connected: false,
        vendor: None,
        unique_number: None,
        printer_id: None,
        blocked: false,
        report_unresolved: false,
        orphaned_receipt: false,
        updated_at: jiff::Timestamp::UNIX_EPOCH,
    }))?;
    writer.submit(Request::Telemetry(false))?;
    assert!(
        completions.recv_timeout(WAIT)?.is_err(),
        "failed status has no success outcome"
    );
    assert_eq!(completions.recv_timeout(WAIT)?, Ok(Outcome::Telemetry));
    assert!(
        !Settings::load(&dir)?
            .ok_or("missing preferences")?
            .telemetry_enabled,
        "a failed status write must not drop the next preference"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A failed Start must not acknowledge or overwrite the worker's saved configuration"
)]
fn failed_start_preserves_connection_and_does_not_block_the_next_reload()
-> Result<(), Box<dyn Error>> {
    let temporary = TempDir::new()?;
    let dir = Arc::new(StateDir::open(temporary.path().to_owned())?);
    let supplied = settings();
    supplied.save(&dir)?;
    let before = fs::read(dir.path().join("settings.json"))?;
    let (completed, completions) = mpsc::channel();
    let writer = Writer::spawn(Arc::clone(&dir), move |completion| {
        let _sent = completed.send(completion);
    })?;
    writer.submit(Request::Save {
        settings: Settings {
            baud_rate: 123,
            ..supplied.clone()
        },
        start: true,
    })?;
    writer.submit(Request::Reload)?;
    assert_eq!(
        completions.recv_timeout(WAIT)?,
        Err("Nieprawidłowa prędkość transmisji.".into())
    );
    assert_eq!(
        completions.recv_timeout(WAIT)?,
        Ok(Outcome::Reload(supplied))
    );
    assert_eq!(fs::read(dir.path().join("settings.json"))?, before);
    Ok(())
}
