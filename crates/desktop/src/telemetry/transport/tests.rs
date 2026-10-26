use std::{
    env,
    error::Error,
    io::{self, BufRead as _, BufReader},
    net::TcpListener,
    process::Command,
    sync::{Mutex, PoisonError, mpsc},
    thread,
    time::Duration,
};

use sentry::{
    ClientOptions, Envelope, Transport, TransportFactory as _, TransportOptions,
    protocol::{EnvelopeItem, Event},
};

use super::DetachedTransport;

const WAIT: Duration = Duration::from_secs(5);

struct Release(Option<mpsc::Sender<()>>);

impl Release {
    fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _disconnected = sender.send(());
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}

struct HeldTransport {
    sent: mpsc::Sender<String>,
    send_release: Mutex<mpsc::Receiver<()>>,
    disposing: mpsc::Sender<()>,
    drop_release: Mutex<mpsc::Receiver<()>>,
    disposed: mpsc::Sender<()>,
}

impl Transport for HeldTransport {
    fn send_envelope(&self, envelope: Envelope) {
        for item in envelope.items() {
            if let EnvelopeItem::Event(event) = item {
                let _receiver_gone = self.sent.send(event.message.clone().unwrap_or_default());
            }
        }
        let _released = self
            .send_release
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv();
    }
}

impl Drop for HeldTransport {
    fn drop(&mut self) {
        let _observed = self.disposing.send(());
        let _released = self
            .drop_release
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv();
        let _observed = self.disposed.send(());
    }
}

struct Fixture {
    transport: DetachedTransport,
    sent: mpsc::Receiver<String>,
    send_release: Release,
    disposing: mpsc::Receiver<()>,
    drop_release: Release,
    disposed: mpsc::Receiver<()>,
}

fn fixture() -> Fixture {
    let (sent, received) = mpsc::channel();
    let (send_release, send_held) = mpsc::channel();
    let (disposing, disposal_started) = mpsc::channel();
    let (drop_release, drop_held) = mpsc::channel();
    let (disposed, disposal_finished) = mpsc::channel();
    let transport = DetachedTransport::spawn(
        move || HeldTransport {
            sent,
            send_release: Mutex::new(send_held),
            disposing,
            drop_release: Mutex::new(drop_held),
            disposed,
        },
        1,
    );
    Fixture {
        transport,
        sent: received,
        send_release: Release(Some(send_release)),
        disposing: disposal_started,
        drop_release: Release(Some(drop_release)),
        disposed: disposal_finished,
    }
}

fn envelope(message: &'static str) -> Envelope {
    let event = Event {
        message: Some(message.into()),
        ..Event::default()
    };
    Envelope::from(event)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn full_submission_queue_drops_excess_without_waiting_for_io() -> Result<(), Box<dyn Error>> {
    let Fixture {
        transport,
        sent,
        mut send_release,
        disposing,
        mut drop_release,
        disposed,
    } = fixture();
    transport.send_envelope(envelope("in flight"));
    assert_eq!(sent.recv_timeout(WAIT)?, "in flight", "I/O is held");
    let (returned, result) = mpsc::channel();
    let caller = thread::Builder::new()
        .name("telemetry-submit-test".into())
        .spawn(move || {
            transport.send_envelope(envelope("queued"));
            transport.send_envelope(envelope("overflow"));
            let _receiver_gone = returned.send(transport);
        })?;

    let transport = result.recv_timeout(WAIT)?;
    send_release.release();
    assert_eq!(sent.recv_timeout(WAIT)?, "queued", "accepted item survives");
    assert!(transport.flush(WAIT), "admitted envelopes are flushed");
    drop(transport);
    disposing.recv_timeout(WAIT)?;
    assert!(sent.try_recv().is_err(), "overflow was never submitted");
    drop_release.release();
    disposed.recv_timeout(WAIT)?;
    assert!(caller.join().is_ok(), "submission caller completed");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn zero_timeout_flush_cannot_wait_for_space_in_a_full_queue() -> Result<(), Box<dyn Error>> {
    let Fixture {
        transport,
        sent,
        mut send_release,
        disposing,
        mut drop_release,
        disposed,
    } = fixture();
    transport.send_envelope(envelope("in flight"));
    assert_eq!(sent.recv_timeout(WAIT)?, "in flight", "I/O is held");
    transport.send_envelope(envelope("queued"));
    let (returned, result) = mpsc::channel();
    let caller = thread::Builder::new()
        .name("telemetry-flush-test".into())
        .spawn(move || {
            let flushed = transport.flush(Duration::ZERO);
            let _receiver_gone = returned.send((flushed, transport));
        })?;

    let (flushed, transport) = result.recv_timeout(WAIT)?;
    assert!(!flushed, "a held transport has not flushed");
    drop(transport);
    send_release.release();
    disposing.recv_timeout(WAIT)?;
    drop_release.release();
    disposed.recv_timeout(WAIT)?;
    assert!(caller.join().is_ok(), "flush caller completed");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn zero_timeout_shutdown_closes_admission_without_waiting_for_io() -> Result<(), Box<dyn Error>> {
    let Fixture {
        transport,
        sent,
        mut send_release,
        disposing,
        mut drop_release,
        disposed,
    } = fixture();
    transport.send_envelope(envelope("in flight"));
    assert_eq!(sent.recv_timeout(WAIT)?, "in flight", "I/O is held");
    transport.send_envelope(envelope("queued"));
    let (returned, result) = mpsc::channel();
    let caller = thread::Builder::new()
        .name("telemetry-shutdown-test".into())
        .spawn(move || {
            let stopped = transport.shutdown(Duration::ZERO);
            transport.send_envelope(envelope("after shutdown"));
            drop(transport);
            let _receiver_gone = returned.send(stopped);
        })?;

    assert!(
        !result.recv_timeout(WAIT)?,
        "held I/O prevents completed shutdown"
    );
    send_release.release();
    disposing.recv_timeout(WAIT)?;
    assert_eq!(
        sent.try_iter().collect::<Vec<_>>(),
        ["queued"],
        "shutdown rejects later admission"
    );
    drop_release.release();
    disposed.recv_timeout(WAIT)?;
    assert!(caller.join().is_ok(), "shutdown caller completed");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn dropping_wrapper_never_waits_for_underlying_disposal() -> Result<(), Box<dyn Error>> {
    let Fixture {
        transport,
        sent,
        mut send_release,
        disposing,
        mut drop_release,
        disposed,
    } = fixture();
    transport.send_envelope(envelope("in flight"));
    assert_eq!(sent.recv_timeout(WAIT)?, "in flight", "I/O is held");
    let (returned, result) = mpsc::channel();
    let caller = thread::Builder::new()
        .name("telemetry-drop-test".into())
        .spawn(move || {
            drop(transport);
            let _receiver_gone = returned.send(());
        })?;

    result.recv_timeout(WAIT)?;
    send_release.release();
    disposing.recv_timeout(WAIT)?;
    assert!(
        disposed.try_recv().is_err(),
        "underlying destructor remains held after wrapper drop"
    );
    drop_release.release();
    disposed.recv_timeout(WAIT)?;
    assert!(caller.join().is_ok(), "drop caller completed");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn shutdown_acknowledges_only_after_underlying_disposal() -> Result<(), Box<dyn Error>> {
    let Fixture {
        transport,
        sent,
        mut send_release,
        disposing,
        mut drop_release,
        disposed,
    } = fixture();
    transport.send_envelope(envelope("in flight"));
    assert_eq!(sent.recv_timeout(WAIT)?, "in flight", "I/O is held");
    send_release.release();
    assert!(transport.flush(WAIT), "no envelope remains pending");
    let (returned, result) = mpsc::channel();
    let caller = thread::Builder::new()
        .name("telemetry-disposal-test".into())
        .spawn(move || {
            let _receiver_gone = returned.send(transport.shutdown(WAIT));
        })?;

    disposing.recv_timeout(WAIT)?;
    assert!(
        result.try_recv().is_err(),
        "success must include underlying disposal"
    );
    drop_release.release();
    disposed.recv_timeout(WAIT)?;
    assert!(
        result.recv_timeout(WAIT)?,
        "fully disposed transport acknowledges success"
    );
    assert!(caller.join().is_ok(), "disposal caller completed");
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn admitted_zero_timeout_shutdown_never_waits_for_acknowledgement() -> Result<(), Box<dyn Error>> {
    let Fixture {
        transport,
        sent,
        mut send_release,
        disposing,
        mut drop_release,
        disposed,
    } = fixture();
    transport.send_envelope(envelope("in flight"));
    assert_eq!(sent.recv_timeout(WAIT)?, "in flight", "I/O is held");
    let (returned, result) = mpsc::channel();
    let caller = thread::Builder::new()
        .name("telemetry-zero-shutdown-test".into())
        .spawn(move || {
            let stopped = transport.shutdown(Duration::ZERO);
            drop(transport);
            let _receiver_gone = returned.send(stopped);
        })?;

    assert!(
        !result.recv_timeout(WAIT)?,
        "held I/O prevents a completed shutdown"
    );
    send_release.release();
    disposing.recv_timeout(WAIT)?;
    drop_release.release();
    disposed.recv_timeout(WAIT)?;
    assert!(
        caller.join().is_ok(),
        "zero-timeout shutdown caller completed"
    );
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions verify behavior while fixture failures propagate"
)]
fn envelopes_follow_the_environment_proxy() -> Result<(), Box<dyn Error>> {
    const CHILD: &str = "WORKSHOP_TELEMETRY_PROXY_CHILD";
    if env::var_os(CHILD).is_some() {
        let options = ClientOptions::new().dsn("https://public@ingest.example.invalid/1");
        let transport = super::HttpTransport.create_transport_with_options(
            TransportOptions::try_from_client_options(&options).ok_or("missing DSN")?,
        );
        transport.send_envelope(envelope("proxied"));
        let _flushed = transport.flush(WAIT);
        return Ok(());
    }
    let proxy = TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}", proxy.local_addr()?);
    let (received, request) = mpsc::channel();
    let _accepting = thread::Builder::new()
        .name("telemetry-proxy-test".into())
        .spawn(move || -> io::Result<()> {
            let (stream, _peer) = proxy.accept()?;
            let mut line = String::new();
            let _read = BufReader::new(stream).read_line(&mut line)?;
            let _receiver_gone = received.send(line);
            Ok(())
        })?;
    #[expect(
        clippy::disallowed_methods,
        reason = "proxy environment is isolated in a child test process"
    )]
    let status = Command::new(env::current_exe()?)
        .args([
            "--exact",
            "telemetry::transport::tests::envelopes_follow_the_environment_proxy",
            "--nocapture",
        ])
        .env(CHILD, "1")
        // ureq prefers ALL_PROXY over HTTPS_PROXY.
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env("HTTPS_PROXY", &address)
        .env("https_proxy", &address)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .status()?;
    assert!(status.success(), "proxied child sends without failing");
    assert!(
        request
            .recv_timeout(WAIT)?
            .starts_with("CONNECT ingest.example.invalid:443 "),
        "diagnostics reach Sentry through the configured proxy"
    );
    Ok(())
}
