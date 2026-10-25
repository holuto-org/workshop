use std::{
    collections::{BTreeSet, VecDeque},
    io,
    sync::{Arc, Mutex, PoisonError},
    thread::Builder,
    time::{Duration, Instant},
};

use fiscal::{
    error::Error,
    link::{Connection, Link, Port},
};

use super::{Session, next_token};
use crate::{
    frame::{self, MAX_FRAME, tests::reply},
    simulator::Simulator,
};

const TIMEOUT: Duration = Duration::from_millis(40);

struct Wire {
    reads: VecDeque<io::Result<Vec<u8>>>,
    sent: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Port for Wire {
    fn write_all(&mut self, bytes: &[u8], _timeout: Duration) -> io::Result<()> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(bytes.to_vec());
        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        let bytes = self
            .reads
            .pop_front()
            .unwrap_or_else(|| Err(io::ErrorKind::TimedOut.into()))?;
        let size = bytes.len().min(buffer.len());
        buffer[..size].copy_from_slice(&bytes[..size]);
        if size < bytes.len() {
            self.reads.push_front(Ok(bytes[size..].to_vec()));
        }
        Ok(size)
    }
}

fn wire(reads: Vec<io::Result<Vec<u8>>>) -> (Session, Arc<Mutex<Vec<Vec<u8>>>>) {
    let sent = Arc::new(Mutex::new(Vec::new()));
    (
        Session::new(Box::new(Wire {
            reads: reads.into(),
            sent: Arc::clone(&sent),
        })),
        sent,
    )
}

#[test]
fn an_interrupted_read_waits_for_the_reply_without_resending_the_command() {
    let (mut session, sent) = wire(vec![
        Err(io::ErrorKind::Interrupted.into()),
        Ok(reply("trinit\t")),
    ]);
    assert_eq!(
        session
            .command("trinit", &[("bm", "0".to_owned())], TIMEOUT)
            .unwrap()
            .fields(),
        &[]
    );
    assert_eq!(
        *sent.lock().unwrap(),
        [frame::encode("trinit", &[("bm", "0".to_owned())]).unwrap()]
    );
}

#[test]
fn a_tokenized_err_from_an_earlier_async_query_is_not_the_current_command_error() {
    let (mut session, _) = wire(vec![
        Ok(reply("ERR\t@0001\t?2036\tcm!sdev\tfd\t")),
        Ok(reply("trend\t")),
    ]);
    assert_eq!(
        session.command("trend", &[], TIMEOUT).unwrap().fields(),
        &[]
    );
}

#[test]
fn split_frames_noise_and_two_replies_per_read_keep_their_boundaries() {
    let first = reply("scnt\tbt17\t");
    let mut second = first[3..].to_vec();
    second.extend(reply("getrealid\tnuDEMO\t"));
    let (mut session, _) = wire(vec![
        Ok(b"noise\x03old\x02junk".to_vec()),
        Ok(first[..3].to_vec()),
        Ok(second),
    ]);
    assert_eq!(
        session.command("scnt", &[], TIMEOUT).unwrap().get("bt"),
        Some("17")
    );
    assert_eq!(
        session
            .command("getrealid", &[], TIMEOUT)
            .unwrap()
            .get("nu"),
        Some("DEMO")
    );
}

#[test]
fn partial_replies_and_silence_time_out_without_a_second_send() {
    for reads in [vec![], vec![Ok(b"\x02trinit\t".to_vec())]] {
        let (mut session, sent) = wire(reads);
        assert!(
            matches!(
                session.command("trinit", &[], TIMEOUT),
                Err(Error::Timeout("trinit"))
            ),
            "no complete reply"
        );
        assert_eq!(
            *sent.lock().unwrap(),
            [frame::encode("trinit", &[]).unwrap()]
        );
    }
}

#[test]
fn an_oversized_unterminated_reply_is_rejected_before_growing_without_bound() {
    let (mut session, _) = wire(vec![Ok(vec![b'a'; MAX_FRAME])]);
    assert_eq!(
        session
            .command("scnt", &[], TIMEOUT)
            .unwrap_err()
            .to_string(),
        "Zbyt długa odpowiedź drukarki."
    );
}

#[test]
fn a_foreign_reply_is_a_protocol_error_not_the_current_commands_success() {
    let (mut session, _) = wire(vec![Ok(reply("scnt\tbt1\t"))]);
    assert_eq!(
        session
            .command("trend", &[], TIMEOUT)
            .unwrap_err()
            .to_string(),
        "Oczekiwano odpowiedzi trend, otrzymano scnt."
    );
}

#[test]
fn tokens_do_not_repeat_before_the_full_ten_thousand_value_cycle() {
    // Start well beyond the old AtomicU16 rollover; the modulo must happen atomically.
    let mut seen = BTreeSet::new();
    let next = super::AtomicU16::new(0);
    for _ in 0..70_000 {
        let value = next_token(&next);
        assert_eq!(value.len(), 4);
        if seen.len() == 10_000 {
            seen.clear();
        }
        assert!(seen.insert(value), "early token repetition");
    }
}

#[test]
fn an_empty_port_is_refused_before_attempting_to_open_any_device() {
    let sim = Simulator::start();
    assert!(
        matches!(
            Session::open(
                sim.link().as_ref(),
                &Connection {
                    port: " ".to_owned(),
                    baud_rate: 9600
                }
            ),
            Err(Error::NoPort)
        ),
        "blank connection"
    );
    assert_eq!(sim.frames(), Vec::<Vec<u8>>::new());
}

fn serial_timeout_and_partial_reads(
    mut peer: Box<dyn serialport::SerialPort>,
    mut session: Session,
) {
    let expected = frame::encode("trinit", &[]).unwrap();
    assert!(
        matches!(
            session.command("trinit", &[], TIMEOUT),
            Err(Error::Timeout("trinit"))
        ),
        "unique_number silence is a timeout"
    );
    let mut received = vec![0; expected.len()];
    io::Read::read_exact(&mut peer, &mut received).unwrap();
    assert_eq!(received, expected);
    let response = reply("scnt\tbt12\t");
    io::Write::write_all(&mut peer, &response[..5]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let handle = Builder::new()
        .name("unique_number reply".to_owned())
        .spawn(move || {
            let mut command = vec![0; frame::encode("scnt", &[]).unwrap().len()];
            io::Read::read_exact(&mut peer, &mut command).unwrap();
            io::Write::write_all(&mut peer, &response[5..]).unwrap();
            peer
        })
        .unwrap();
    let _deadline = session.send("scnt", &[], TIMEOUT).unwrap();
    assert_eq!(
        session.receive("scnt", deadline).unwrap().get("bt"),
        Some("12")
    );
    let mut peer = handle.join().unwrap();
    io::Write::write_all(&mut peer, b"\x02scnt\tbt").unwrap();
    assert!(
        matches!(
            session.command("scnt", &[], TIMEOUT),
            Err(Error::Timeout("scnt"))
        ),
        "partial frame times out"
    );
}

#[cfg(unix)]
mod pty {
    use super::{Connection, Link, Session, TIMEOUT, serial_timeout_and_partial_reads};
    use fiscal::link::SerialLink;
    use serialport::{SerialPort, TTYPort};

    fn pair() -> (TTYPort, Session, String) {
        let (mut master, slave) = TTYPort::pair().unwrap();
        let path = slave.name().unwrap();
        drop(slave);
        master.set_timeout(TIMEOUT).unwrap();
        let session = Session::open(
            &SerialLink,
            &Connection {
                port: path.clone(),
                baud_rate: 9600,
            },
        )
        .unwrap();
        (master, session, path)
    }

    #[test]
    fn opening_a_real_terminal_uses_exclusivity_and_releases_it_on_drop() {
        let (_master, session, path) = pair();
        assert!(
            SerialLink.open(&path, 9600).is_err(),
            "second unique_number open must fail"
        );
        drop(session);
        assert!(
            SerialLink.open(&path, 9600).is_ok(),
            "drop releases unique_number ownership"
        );
    }

    #[test]
    fn the_native_open_path_preserves_partial_reads_and_maps_serial_timeouts() {
        let (peer, session, _) = pair();
        serial_timeout_and_partial_reads(Box::new(peer), session);
    }
}

#[cfg(windows)]
mod com_pair {
    use super::{Connection, Link, Session, TIMEOUT, serial_timeout_and_partial_reads};
    use fiscal::link::SerialLink;

    #[test]
    fn real_com_ports_are_exclusive_and_preserve_partial_reads_and_timeouts() {
        let configured = std::env::var("HOLUTO_TEST_COM_PAIR")
            .expect("HOLUTO_TEST_COM_PAIR must name the signed com0com pair, e.g. COM10,COM11");
        let (driver, peer) = configured
            .split_once(',')
            .expect("HOLUTO_TEST_COM_PAIR must contain two COM names");
        let peer = serialport::new(peer, 9600).timeout(TIMEOUT).open().unwrap();
        let session = Session::open(
            &SerialLink,
            &Connection {
                port: driver.to_owned(),
                baud_rate: 9600,
            },
        )
        .unwrap();
        assert!(
            SerialLink.open(driver, 9600).is_err(),
            "second COM open must fail"
        );
        serial_timeout_and_partial_reads(peer, session);
        assert!(
            SerialLink.open(driver, 9600).is_ok(),
            "drop releases COM ownership"
        );
    }
}
