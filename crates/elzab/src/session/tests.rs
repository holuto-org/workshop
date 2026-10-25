use super::*;
use crate::simulator::{Fault, Simulator};
use fiscal::link::Candidate;
use std::sync::{Arc, Mutex, PoisonError};
fn open(sim: &Simulator) -> Session {
    Session::open(
        sim.link().as_ref(),
        &Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    )
    .unwrap()
}
#[test]
fn transport_rejection_must_be_single_frame() {
    let sim = Simulator::start();
    sim.fault(
        "LOR",
        Fault::Reply(frame::encode("1\x1dSL\x1d2\x1d2", b"").unwrap()),
    );
    assert!(matches!(
        open(&sim).command("LOR", "C0", Duration::from_secs(3)),
        Err(Error::Protocol(_))
    ));
}
#[test]
fn fragmented_and_multipart_responses_are_assembled_without_ack_or_retry() {
    let sim = Simulator::start();
    sim.set_fragment_size(1);
    let mut reply = frame::encode("1\x1dSA\x1d1\x1d0", b"\x1dBABC").unwrap();
    reply.extend(frame::encode("1\x1dSA\x1d2\x1d2", b"DEF").unwrap());
    sim.fault("FNR", Fault::Reply(reply));
    assert_eq!(
        open(&sim)
            .command("FNR", "", Duration::from_secs(3))
            .unwrap(),
        "\x1dBABCDEF"
    );
    assert_eq!(sim.frames().len(), 1);
}
#[test]
fn poison_prevents_any_further_send() {
    for fault in [
        Fault::LoseReply,
        Fault::CorruptReply,
        Fault::Disconnect,
        Fault::Reply(frame::encode("1", b"nonsense").unwrap()),
    ] {
        let sim = Simulator::start();
        sim.fault("XTR", fault);
        let mut session = open(&sim);
        assert!(session.command("XTR", "", Duration::from_secs(3)).is_err());
        assert!(matches!(
            session.command("FNR", "", Duration::from_secs(3)),
            Err(Error::NotReady(_))
        ));
        assert_eq!(sim.frames().len(), 1);
    }
}
#[test]
fn every_transport_status_is_decoded_without_replay() {
    for status in [
        "SB", "SC", "SE", "SF", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN",
    ] {
        let sim = Simulator::start();
        sim.fault("LOR", Fault::Transport(status.to_owned()));
        assert!(matches!(
            open(&sim).command("LOR", "C0", Duration::from_secs(3)),
            Err(Error::Device { rejected: true, .. })
        ));
        assert_eq!(sim.frames().len(), 1);
    }
}
#[test]
fn malformed_headers_never_become_transport_rejections() {
    for header in [
        "1\x1dSD",
        "1\x1dSL\x1d0",
        "1\x1dSL\x1d1\x1d1\x1dZIP",
        "1\x1dSL\x1d1\x1d1\x1dNDT\x1dextra",
    ] {
        let sim = Simulator::start();
        sim.fault("LOR", Fault::Reply(frame::encode(header, b"").unwrap()));
        assert!(matches!(
            open(&sim).command("LOR", "C0", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
    }
}

#[test]
fn reply_with_default_header_fields_matches_command_one() {
    for header in ["\x1d", "\x1d\x1d\x1d\x1d", ""] {
        let sim = Simulator::start();
        sim.fault(
            "FNR",
            Fault::Reply(frame::encode(header, b"\x1dBABC").unwrap()),
        );
        assert_eq!(
            open(&sim)
                .command("FNR", "", Duration::from_secs(3))
                .unwrap(),
            "\x1dBABC"
        );
        assert_eq!(sim.frames().len(), 1);
    }
}

#[test]
fn inconsistent_multipart_totals_are_rejected() {
    for headers in [
        vec!["1\x1dSA\x1d1\x1d3", "1\x1dSA\x1d2\x1d2"],
        vec![
            "1\x1dSA\x1d1\x1d2",
            "1\x1dSA\x1d2\x1d3",
            "1\x1dSA\x1d3\x1d3",
        ],
        vec![
            "1\x1dSA\x1d1\x1d3",
            "1\x1dSA\x1d2\x1d0",
            "1\x1dSA\x1d3\x1d3",
        ],
        vec![
            "1\x1dSA\x1d1\x1d0",
            "1\x1dSA\x1d2\x1d3",
            "1\x1dSA\x1d3\x1d3",
        ],
    ] {
        let sim = Simulator::start();
        let mut raw = Vec::new();
        for header in headers {
            raw.extend(frame::encode(header, b"").unwrap());
        }
        sim.fault("LCL", Fault::Reply(raw));
        let mut session = open(&sim);
        assert!(matches!(
            session.command("LCL", "RA1,00", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            session.command("FNR", "", Duration::from_secs(3)),
            Err(Error::NotReady(_))
        ));
        assert_eq!(sim.frames().len(), 1);
    }
}

#[test]
fn a_declared_multipart_total_is_preserved_until_its_final_frame() {
    let sim = Simulator::start();
    let mut reply = frame::encode("1\x1dSA\x1d1\x1d2", b"\x1dBABC").unwrap();
    reply.extend(frame::encode("1\x1dSA\x1d2\x1d2", b"DEF").unwrap());
    sim.fault("FNR", Fault::Reply(reply));
    assert_eq!(
        open(&sim)
            .command("FNR", "", Duration::from_secs(3))
            .unwrap(),
        "\x1dBABCDEF"
    );
    assert_eq!(sim.frames().len(), 1);
}

/// The real simulator behind a chosen time scale, recording each write's deadline.
#[derive(Debug)]
struct Recording {
    inner: Arc<dyn Link>,
    scale: u32,
    deadlines: Arc<Mutex<Vec<Duration>>>,
}
impl Link for Recording {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Ok(Box::new(RecordingPort {
            inner: self.inner.open(port, baud_rate)?,
            deadlines: Arc::clone(&self.deadlines),
        }))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.inner.candidates()
    }

    fn time_scale(&self) -> u32 {
        self.scale
    }
}
struct RecordingPort {
    inner: Box<dyn Port>,
    deadlines: Arc<Mutex<Vec<Duration>>>,
}
impl Port for RecordingPort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.deadlines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(timeout);
        self.inner.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.inner.read(buffer, timeout)
    }
}

#[test]
fn scaled_deadlines_keep_a_scheduling_margin_and_real_links_keep_the_exact_one() {
    // A 3 ms query deadline expires whenever a loaded test host deschedules the thread.
    for (scale, deadline) in [
        (1000, Duration::from_millis(100)),
        (1, Duration::from_secs(3)),
    ] {
        let sim = Simulator::start();
        let deadlines = Arc::new(Mutex::new(Vec::new()));
        let link = Recording {
            inner: sim.link(),
            scale,
            deadlines: Arc::clone(&deadlines),
        };
        let mut session = Session::open(
            &link,
            &Connection {
                port: sim.address().to_owned(),
                baud_rate: 9600,
            },
        )
        .unwrap();
        assert_eq!(
            session.command("FNR", "", Duration::from_secs(3)).unwrap(),
            "\x1dBABC1234567890"
        );
        assert_eq!(*deadlines.lock().unwrap(), [deadline], "scale {scale}");
    }
}
