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
fn fragmented_and_coalesced_multipart_responses_need_no_ack_or_retry() {
    for fragment in [1, usize::MAX] {
        for maximum in [0, 2] {
            let sim = Simulator::start();
            sim.set_fragment_size(fragment);
            let mut reply =
                frame::encode(&format!("1\x1dSA\x1d1\x1d{maximum}"), b"\x1dBABC").unwrap();
            reply.extend(frame::encode("1\x1dSA\x1d2\x1d2", b"DEF").unwrap());
            sim.fault("FNR", Fault::Reply(reply));
            assert_eq!(
                open(&sim)
                    .command("FNR", "", Duration::from_secs(3))
                    .unwrap(),
                "\x1dBABCDEF"
            );
            assert_eq!(sim.frames(), [frame::encode("1", b"FNR").unwrap()]);
        }
    }
}
#[test]
fn poison_prevents_any_further_send() {
    for (fault, kind) in [
        (Fault::LoseReply, "timeout"),
        (Fault::CorruptReply, "protocol"),
        (Fault::Disconnect, "eof"),
        (
            Fault::Reply(frame::encode("1", b"nonsense").unwrap()),
            "protocol",
        ),
    ] {
        let sim = Simulator::start();
        sim.fault("XTR", fault);
        let mut session = open(&sim);
        let error = session
            .command("XTR", "", Duration::from_secs(3))
            .unwrap_err();
        assert!(match kind {
            "timeout" => matches!(error, Error::Timeout("XTR")),
            "eof" =>
                matches!(error, Error::Io(error) if error.kind() == io::ErrorKind::UnexpectedEof),
            _ => matches!(error, Error::Protocol(message)
                if message == "Nieprawidłowa ramka ElzabSTX."),
        });
        assert!(matches!(
            session.command("FNR", "", Duration::from_secs(3)),
            Err(Error::NotReady(message))
                if message == "Poprzednia odpowiedź Elzab jest niepewna. Zamknij sesję."
        ));
        assert_eq!(sim.frames(), [frame::encode("1", b"XTR").unwrap()]);
    }
}
#[test]
fn every_transport_status_is_decoded_without_replay() {
    for status in [
        "SB", "SC", "SE", "SF", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN",
    ] {
        let sim = Simulator::start();
        sim.fault("LOR", Fault::Transport(status.to_owned()));
        let mut session = open(&sim);
        let expected_detail =
            format!("Elzab odrzucił transmisję LOR: {status}. Polecenie nie zostało ponowione.");
        assert!(matches!(
            session.command("LOR", "C0", Duration::from_secs(3)),
            Err(Error::Device { code, command, detail, rejected: true })
                if code == u32::from(status.bytes().last().unwrap())
                    && command == "LOR" && detail == expected_detail
        ));
        assert_eq!(
            session.command("FNR", "", Duration::from_secs(3)).unwrap(),
            "\x1dBABC1234567890"
        );
        assert_eq!(
            sim.frames(),
            [
                frame::encode("1", b"LOR\x1dC0").unwrap(),
                frame::encode("2", b"FNR").unwrap(),
            ]
        );
        assert!(!sim.transaction_open());
    }
}
#[test]
fn malformed_headers_never_become_transport_rejections() {
    for header in [
        "1\x1dSD",
        "1\x1dSL\x1d0",
        "1\x1dSL\x1d2\x1d2",
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
        assert_eq!(sim.frames(), [frame::encode("1", b"FNR").unwrap()]);
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
        assert_eq!(
            sim.frames(),
            [frame::encode("1", b"LCL\x1dRA1,00").unwrap()]
        );
    }
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

#[test]
fn rejected_local_text_sends_nothing_and_does_not_consume_an_id() {
    let sim = Simulator::start();
    let mut session = open(&sim);
    for (command, fields, message) in [
        ("LOR\x02", "", "Niedozwolony znak w poleceniu ElzabSTX."),
        (
            "LIT",
            "GAOlej\x1cX",
            "Niedozwolony znak w poleceniu ElzabSTX.",
        ),
        ("LIT", "GA😀", "Polecenie nie mieści się w CP1250."),
    ] {
        assert!(
            matches!(session.command(command, fields, Duration::from_secs(3)),
            Err(Error::Invalid(actual)) if actual == message)
        );
        assert_eq!(sim.frames(), Vec::<Vec<u8>>::new());
    }
    assert_eq!(
        session.command("FNR", "", Duration::from_secs(3)).unwrap(),
        "\x1dBABC1234567890"
    );
    assert_eq!(sim.frames(), [frame::encode("1", b"FNR").unwrap()]);
}

#[test]
fn fresh_ids_ignore_late_replies_and_complete_device_errors_allow_the_next_command() {
    let sim = Simulator::start();
    let mut session = open(&sim);
    sim.fault("FNR", Fault::Device(450));
    assert!(matches!(session.command("FNR", "", Duration::from_secs(3)),
        Err(Error::Device { code: 450, command, detail, rejected: false })
        if command == "FNR" && detail == "Błąd drukarki Elzab 450 podczas FNR."));
    let mut reply = frame::encode("1", b"123").unwrap();
    reply.extend(frame::encode("2", b"\x1dBSECOND").unwrap());
    sim.fault("FNR", Fault::Reply(reply));
    assert_eq!(
        session.command("FNR", "", Duration::from_secs(3)).unwrap(),
        "\x1dBSECOND"
    );
    assert_eq!(
        session.command("FNR", "", Duration::from_secs(3)).unwrap(),
        "\x1dBABC1234567890"
    );
    assert_eq!(
        sim.frames(),
        [
            frame::encode("1", b"FNR").unwrap(),
            frame::encode("2", b"FNR").unwrap(),
            frame::encode("3", b"FNR").unwrap(),
        ]
    );
}

#[test]
fn incomplete_invalid_and_foreign_only_replies_poison_before_another_write() {
    let mut partial = frame::encode("1", b"").unwrap();
    partial.truncate(partial.len() - 1);
    for (raw, timeout) in [
        (partial, true),
        (frame::encode("2", b"").unwrap(), true),
        (frame::encode("1", b"\x1dB\x81").unwrap(), false),
        (frame::encode("1", b"-1").unwrap(), false),
        (frame::encode("1\x1dSL", b"450").unwrap(), false),
        (frame::encode("1\x1dSA\x1d2\x1d2", b"").unwrap(), false),
        (frame::encode("1\x1dSA\x1d1\x1d0", b"").unwrap(), true),
        (vec![frame::STX; frame::MAX_FRAME], false),
    ] {
        let sim = Simulator::start();
        sim.fault("FNR", Fault::Reply(raw));
        let mut session = open(&sim);
        let error = session
            .command("FNR", "", Duration::from_secs(3))
            .unwrap_err();
        assert!(if timeout {
            matches!(error, Error::Timeout("FNR"))
        } else {
            matches!(error, Error::Protocol(message) if message == "Nieprawidłowa ramka ElzabSTX.")
        });
        assert!(matches!(session.command("XTR", "", Duration::from_secs(3)),
            Err(Error::NotReady(message))
            if message == "Poprzednia odpowiedź Elzab jest niepewna. Zamknij sesję."));
        assert_eq!(sim.frames(), [frame::encode("1", b"FNR").unwrap()]);
    }
}

#[test]
fn multipart_reply_bound_is_exact_and_frame_numbers_cannot_skip_or_repeat() {
    for size in [65_536, 65_537] {
        let sim = Simulator::start();
        let mut body = vec![b'A'; size];
        body[0] = 0x1D;
        let chunks: Vec<_> = body.chunks(1000).collect();
        let mut reply = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let number = index + 1;
            let maximum = if number == chunks.len() { number } else { 0 };
            reply.extend(
                frame::encode(&format!("1\x1dSA\x1d{number}\x1d{maximum}"), chunk).unwrap(),
            );
        }
        sim.fault("FNR", Fault::Reply(reply));
        let mut session = open(&sim);
        let result = session.command("FNR", "", Duration::from_secs(3));
        if size == 65_536 {
            assert_eq!(result.unwrap().as_bytes(), body);
        } else {
            assert!(matches!(result, Err(Error::Protocol(_))));
            assert!(matches!(
                session.command("XTR", "", Duration::from_secs(3)),
                Err(Error::NotReady(_))
            ));
        }
        assert_eq!(sim.frames(), [frame::encode("1", b"FNR").unwrap()]);
    }
    for number in [1, 3] {
        let sim = Simulator::start();
        let mut reply = frame::encode("1\x1dSA\x1d1\x1d3", b"").unwrap();
        reply.extend(frame::encode(&format!("1\x1dSA\x1d{number}\x1d3"), b"").unwrap());
        sim.fault("FNR", Fault::Reply(reply));
        assert!(matches!(
            open(&sim).command("FNR", "", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
        assert_eq!(sim.frames(), [frame::encode("1", b"FNR").unwrap()]);
    }
}
