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
fn session_never_retries_a_silent_command() {
    let sim = Simulator::start();
    sim.fault("^u", Fault::LoseReply);
    let mut session = open(&sim);
    assert!(matches!(
        session.command("^u", Duration::from_secs(1)),
        Err(Error::Timeout("Novitus"))
    ));
    assert!(matches!(
        session.command("^u", Duration::from_secs(1)),
        Err(Error::Protocol(_))
    ));
    assert_eq!(
        sim.frames(),
        vec![b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP^uD4\x1b\\".to_vec()]
    );
    assert_eq!(session.realtime(DLE).unwrap(), 0x74);
}

#[test]
fn realtime_status_bits_decode_independently_through_the_session() {
    for (byte, online, paper_or_battery, mechanism_error) in [
        (0x70, false, false, false),
        (0x74, true, false, false),
        (0x72, false, true, false),
        (0x71, false, false, true),
    ] {
        let sim = Simulator::start();
        sim.set_status(byte, 0x6D, "@s0;0;0;00000000;");
        let wire = open(&sim).realtime(DLE).unwrap();
        assert_eq!(
            DleStatus::decode(wire).unwrap(),
            DleStatus {
                online,
                paper_or_battery,
                mechanism_error
            }
        );
    }
    for (byte, fiscal, transaction) in [
        (0x65, false, false),
        (0x6D, true, false),
        (0x67, false, true),
    ] {
        let sim = Simulator::start();
        sim.set_status(0x74, byte, "@s0;0;0;00000000;");
        let wire = open(&sim).realtime(ENQ).unwrap();
        assert_eq!(
            EnqStatus::decode(wire).unwrap(),
            EnqStatus {
                fiscal,
                transaction
            }
        );
    }
}

#[test]
fn initializes_once_and_reads_queries_and_realtime() {
    let sim = Simulator::start();
    let mut session = open(&sim);
    assert_eq!(
        session
            .command("30#v", Duration::from_secs(3))
            .unwrap()
            .body,
        "1#RNOVITUS HD ONLINE/1.60"
    );
    assert_eq!(
        session
            .command("50$i", Duration::from_secs(3))
            .unwrap()
            .body,
        "50$IPC1\r"
    );
    assert_eq!(
        session
            .command("0;1^c5/5/", Duration::from_secs(3))
            .unwrap()
            .body,
        "1^c5\r1\r"
    );
    assert_eq!(
        session.command("#c", Duration::from_secs(3)).unwrap().body,
        "1#C26;10;24;12;0;0"
    );
    assert_eq!(session.realtime(DLE).unwrap(), 0x74);
    assert_eq!(session.realtime(ENQ).unwrap(), 0x6D);
    assert_eq!(
        session.extended_status().unwrap(),
        ExtendedStatus {
            last_error: 0,
            flags_wire: [0; 4]
        }
    );
    assert_eq!(
        sim.frames()
            .iter()
            .filter(|frame| frame.starts_with(b"\x1bP3#e"))
            .count(),
        1
    );
}

#[test]
fn mismatched_ack_is_not_success_and_large_error_is_not_rejection() {
    let sim = Simulator::start();
    sim.fault("^u", Fault::Reply(b"\x1bP0#Z$h\x1b\\".to_vec()));
    assert!(matches!(
        open(&sim).command("^u", Duration::from_secs(3)),
        Err(Error::Protocol(_))
    ));
    let sim = Simulator::start();
    sim.fault("^u", Fault::Reply(b"\x1bP1530#Z^u\x1b\\".to_vec()));
    assert!(matches!(
        open(&sim).command("^u", Duration::from_secs(3)),
        Err(Error::Device { code: 1530, command, rejected: false, .. }) if command == "^u"
    ));
}

#[test]
fn initialization_ack_is_optional() {
    let sim = Simulator::start();
    sim.fault("#e", Fault::LoseReply);
    assert_eq!(
        open(&sim)
            .command("^u", Duration::from_secs(3))
            .unwrap()
            .body,
        "1#UWarsztat Łódź"
    );
}

#[test]
fn bad_status_and_corrupt_reply_fail_closed() {
    let sim = Simulator::start();
    sim.set_status(0x74, 0x6D, "@s1530;12;34;12345678;");
    assert_eq!(
        open(&sim).extended_status().unwrap(),
        ExtendedStatus {
            last_error: 1530,
            flags_wire: [0x12, 0x34, 0x56, 0x78]
        }
    );
    for invalid in [
        "@s0;0;0;0000000;",
        "@s0;0;0;GG000000;",
        "@s0;0;0;00000000",
        "@s0;0;0;00000000;1;",
    ] {
        let sim = Simulator::start();
        sim.set_status(0x74, 0x6D, invalid);
        assert!(
            matches!(open(&sim).extended_status(), Err(Error::Protocol(_))),
            "{invalid}"
        );
    }
    let sim = Simulator::start();
    sim.fault("^u", Fault::CorruptReply);
    assert!(matches!(
        open(&sim).command("^u", Duration::from_secs(3)),
        Err(Error::Protocol(_))
    ));
}

#[test]
fn header_ack_marker_is_not_a_completion_frame() {
    let sim = Simulator::start();
    sim.set_header("Warsztat #Z^u");
    assert_eq!(
        open(&sim)
            .command("^u", Duration::from_secs(3))
            .unwrap()
            .body,
        "1#UWarsztat #Z^u"
    );
}

#[test]
fn raw_status_families_are_demultiplexed() {
    let sim = Simulator::start();
    sim.fault(
        "#v",
        Fault::Reply(b"\x6d\x1bP1#RNOVITUS HD ONLINE/1.60\x1b\\".to_vec()),
    );
    let mut session = open(&sim);
    assert_eq!(
        session
            .command("30#v", Duration::from_secs(3))
            .unwrap()
            .body,
        "1#RNOVITUS HD ONLINE/1.60"
    );
    assert_eq!(session.realtime(DLE).unwrap(), 0x74);
}

#[test]
fn invalid_raw_status_is_not_ignored_before_a_successful_reply() {
    let sim = Simulator::start();
    sim.fault(
        "#v",
        Fault::Reply(b"\0\x1bP1#RNOVITUS HD ONLINE/1.60\x1b\\".to_vec()),
    );
    assert!(matches!(
        open(&sim).command("30#v", Duration::from_secs(3)),
        Err(Error::Protocol(_))
    ));
}

#[test]
fn eof_and_partial_frames_never_complete_or_resend() {
    for (fault, eof) in [
        (Fault::Disconnect, true),
        (Fault::Reply(b"\x1bP1#U".to_vec()), false),
    ] {
        let sim = Simulator::start();
        sim.fault("^u", fault);
        let mut session = open(&sim);
        let error = session.command("^u", Duration::from_secs(3)).unwrap_err();
        if eof {
            assert!(
                matches!(error, Error::Io(source) if source.kind() == io::ErrorKind::UnexpectedEof)
            );
        } else {
            assert!(matches!(error, Error::Timeout("Novitus")));
        }
        assert_eq!(
            sim.frames(),
            [b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP^uD4\x1b\\".to_vec()]
        );
        assert!(matches!(
            session.command("^u", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
        assert_eq!(
            sim.frames(),
            [b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP^uD4\x1b\\".to_vec()]
        );
    }
}

#[test]
fn config_reply_with_several_options_parses_every_pair() {
    let sim = Simulator::start();
    sim.set_config_option(92, "3");
    let reply = open(&sim)
        .command("0;1^c91/92/", Duration::from_secs(3))
        .unwrap();
    assert_eq!(reply.body, "2^c91\r1\r92\r3\r");
    assert_eq!(
        reply.options,
        vec![(91, "1".to_owned()), (92, "3".to_owned())]
    );
}

#[test]
fn config_reply_rejects_malformed_counts_and_pairs() {
    for body in [
        "1^c91\r1\r92\r1\r",
        "3^c91\r1\r92\r1\r",
        "2^c91\r1\r92\r",
        "2^c91\r1\r92\r1",
        "+2^c91\r1\r92\r1\r",
        "2^c91\r1\rinvalid\r1\r",
        "0^c91\r1\r",
        "^c91\r1\r",
        "1^c91\r1\r\r",
    ] {
        let sim = Simulator::start();
        let mut raw = frame::encode(body.as_bytes());
        raw.extend_from_slice(b"\x1bP0#Z^c\x1b\\");
        sim.fault("^c", Fault::Reply(raw));
        assert!(
            matches!(
                open(&sim).command("0;1^c91/92/", Duration::from_secs(3)),
                Err(Error::Protocol(_))
            ),
            "{body:?}"
        );
    }
}

#[test]
fn full_config_read_returns_all_counted_options() {
    let sim = Simulator::start();
    sim.set_dialect("0");
    let reply = open(&sim).command("^c", Duration::from_secs(3)).unwrap();
    assert_eq!(
        reply.options,
        vec![
            (5, "0".to_owned()),
            (91, "1".to_owned()),
            (92, "1".to_owned()),
            (93, "1".to_owned()),
            (94, "1".to_owned()),
        ]
    );
}

#[test]
fn operator_cap_expiry_prevents_even_status_writes() {
    let sim = Simulator::start();
    let mut session = open(&sim);
    let cap = Instant::now();
    assert!(matches!(
        session.extended_status_until(cap),
        Err(Error::Timeout("Novitus"))
    ));
    assert!(matches!(
        session.realtime_until(DLE, cap),
        Err(Error::Timeout("Novitus"))
    ));
    assert!(
        sim.frames()
            .iter()
            .all(|raw| raw.starts_with(&frame::START))
    );
}

#[test]
fn invalid_outgoing_text_and_oversized_commands_send_nothing_and_leave_session_usable() {
    for (body, expected) in [
        (
            "^u\n".to_owned(),
            "Niedozwolony znak sterujący w poleceniu Novitus.",
        ),
        (
            "^u\u{1b}".to_owned(),
            "Niedozwolony znak sterujący w poleceniu Novitus.",
        ),
        ("^u😀".to_owned(), "Tekst nie mieści się w Windows-1250."),
        (
            "not-a-command".to_owned(),
            "Nieprawidłowe polecenie Novitus.",
        ),
        (
            format!("^u\r{}", "A".repeat(frame::MAX_FRAME)),
            "Polecenie Novitus jest zbyt długie.",
        ),
    ] {
        let sim = Simulator::start();
        let mut session = open(&sim);
        assert!(matches!(
            session.command(&body, Duration::from_secs(3)),
            Err(Error::Invalid(message)) if message == expected
        ));
        assert_eq!(
            session
                .command("30#v", Duration::from_secs(3))
                .unwrap()
                .body,
            "1#RNOVITUS HD ONLINE/1.60"
        );
        assert_eq!(
            sim.frames(),
            [b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP30#v\x1b\\".to_vec()]
        );
    }
}

#[test]
fn undefined_cp1250_bytes_and_controls_in_checked_replies_block_further_commands() {
    for byte in [0x81, 0x83, 0x88, 0x90, 0x98, 0, b'\n', 0x7F] {
        let sim = Simulator::start();
        let mut reply = frame::encode(&[b'1', b'#', b'U', byte]);
        reply.extend(frame::query(b"0#Z^u"));
        sim.fault("^u", Fault::Reply(reply));
        let mut session = open(&sim);
        assert!(
            matches!(
                session.command("^u", Duration::from_secs(3)),
                Err(Error::Protocol(_))
            ),
            "{byte:02X}"
        );
        assert!(matches!(
            session.command("30#v", Duration::from_secs(3)),
            Err(Error::Protocol(message))
                if message == "Sesja Novitus ma nierozstrzygnięte polecenie."
        ));
        assert_eq!(
            sim.frames(),
            [b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP^uD4\x1b\\".to_vec()]
        );
    }
}

#[test]
fn data_and_completion_may_arrive_in_either_order_and_in_single_byte_fragments() {
    for ack_first in [false, true] {
        let sim = Simulator::start();
        sim.set_fragment_size(1);
        let data = frame::encode(b"1#UWarsztat");
        let ack = frame::query(b"0#Z^u");
        let reply = if ack_first {
            [ack, data].concat()
        } else {
            [data, ack].concat()
        };
        sim.fault("^u", Fault::Reply(reply));
        let mut session = open(&sim);
        assert_eq!(
            session.command("^u", Duration::from_secs(3)).unwrap().body,
            "1#UWarsztat"
        );
        assert_eq!(
            session.command("#c", Duration::from_secs(3)).unwrap().body,
            "1#C26;10;24;12;0;0"
        );
    }
}

#[test]
fn excess_status_noise_and_oversized_incoming_frames_fail_closed() {
    for reply in [
        [vec![0x74; 65], frame::query(b"1#RNOVITUS HD ONLINE/1.60")].concat(),
        frame::query(&vec![b'A'; frame::MAX_FRAME]),
    ] {
        let sim = Simulator::start();
        sim.fault("#v", Fault::Reply(reply));
        let mut session = open(&sim);
        assert!(matches!(
            session.command("30#v", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            session.command("30#v", Duration::from_secs(3)),
            Err(Error::Protocol(message))
                if message == "Sesja Novitus ma nierozstrzygnięte polecenie."
        ));
        assert_eq!(
            sim.frames(),
            [b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP30#v\x1b\\".to_vec()]
        );
    }
}

#[test]
fn maximum_size_command_is_sent_instead_of_rejected_at_the_boundary() {
    let sim = Simulator::start();
    let mut session = open(&sim);
    let body = format!("^u\r{}", "A".repeat(frame::MAX_FRAME - 9));
    assert!(matches!(
        session.command(&body, Duration::from_secs(3)),
        Err(Error::Device { code: 82, command, .. }) if command == "^u"
    ));
    let expected = [
        b"\x1bP^u\r".as_slice(),
        &vec![b'A'; frame::MAX_FRAME - 9],
        b"98\x1b\\",
    ]
    .concat();
    assert_eq!(sim.frames().last(), Some(&expected));
}

#[test]
fn debug_exposes_buffer_size_and_blocking_without_partial_customer_text() {
    let sim = Simulator::start();
    sim.fault("^u", Fault::Reply(b"\x1bP1#USECRET-CANARY".to_vec()));
    let mut session = open(&sim);
    assert!(matches!(
        session.command("^u", Duration::from_secs(3)),
        Err(Error::Timeout("Novitus"))
    ));
    assert_eq!(
        format!("{session:?}"),
        "Session { pending: 18, blocked: true, .. }"
    );
}

#[derive(Debug)]
struct InterruptedLink(Arc<dyn Link>);

impl Link for InterruptedLink {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Ok(Box::new(InterruptedPort {
            inner: self.0.open(port, baud_rate)?,
            interrupted: false,
        }))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.0.candidates()
    }

    fn time_scale(&self) -> u32 {
        self.0.time_scale()
    }
}

struct InterruptedPort {
    inner: Box<dyn Port>,
    interrupted: bool,
}

impl Port for InterruptedPort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.inner.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.inner.read(buffer, timeout)
    }
}

#[test]
fn interrupted_transport_read_continues_receiving_without_resending() {
    let sim = Simulator::start();
    let link = InterruptedLink(sim.link());
    let mut session = Session::open(
        &link,
        &Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    )
    .unwrap();
    assert_eq!(
        session
            .command("30#v", Duration::from_secs(3))
            .unwrap()
            .body,
        "1#RNOVITUS HD ONLINE/1.60"
    );
    assert_eq!(
        sim.frames(),
        [b"\x1bP3#e8A\x1b\\".to_vec(), b"\x1bP30#v\x1b\\".to_vec()]
    );
}

#[test]
fn status_query_schemas_do_not_apply_to_other_hash_s_commands() {
    let sim = Simulator::start();
    let mut reply = frame::encode(
        b"2#X0;1;0;1;1;0;26;10;24/23/8/5/0/98.99/99.99/99.99/0/0/0/0/0/0/0/0/0/ABC1234567890",
    );
    reply.extend(frame::query(b"0#Z#s"));
    sim.fault("#s", Fault::Reply(reply));
    assert!(matches!(
        open(&sim).command("99#s", Duration::from_secs(3)),
        Err(Error::Protocol(_))
    ));
}

#[test]
fn clock_and_fiscal_memory_refusals_keep_their_operator_explanations() {
    for (command, code, detail) in [
        (
            "$h",
            1,
            "Drukarka Novitus zwróciła błąd 1 ($h). Nie zainicjowano zegara drukarki.",
        ),
        (
            "$h",
            6,
            "Drukarka Novitus zwróciła błąd 6 ($h). Błąd pamięci fiskalnej. Wezwij serwis.",
        ),
        (
            "$h",
            40,
            "Drukarka Novitus zwróciła błąd 40 ($h). Brak nagłówka w pamięci drukarki.",
        ),
        (
            "#r",
            7,
            "Drukarka Novitus zwróciła błąd 7 (#r). Data drukarki jest wcześniejsza niż ostatni \
             zapis albo zmieniła się przed raportem.",
        ),
    ] {
        let sim = Simulator::start();
        sim.fault(command, Fault::Refuse(code));
        let error = open(&sim)
            .command(
                if command == "$h" {
                    "0$h"
                } else {
                    "1;26;10;24#r"
                },
                Duration::from_secs(3),
            )
            .unwrap_err();
        assert!(matches!(
            &error,
            Error::Device { code: actual, command: answered, .. }
                if *actual == code && answered == command
        ));
        assert_eq!(error.to_string(), detail);
        assert_eq!(sim.receipts(), 0);
    }
}

#[test]
fn a_status_of_the_wrong_family_cannot_satisfy_a_realtime_request() {
    let sim = Simulator::start();
    sim.set_status(0x6D, 0x6D, "@s0;0;0;00000000;");
    let mut session = open(&sim);
    assert!(matches!(
        session.realtime(DLE),
        Err(Error::Timeout("Novitus"))
    ));
    sim.set_status(0x74, 0x6D, "@s0;0;0;00000000;");
    assert_eq!(session.realtime(ENQ).unwrap(), 0x6D);
}

#[test]
fn scaled_and_unscaled_deadlines_reach_the_real_transport_unchanged() {
    #[derive(Debug)]
    struct Observe {
        inner: Arc<dyn Link>,
        scale: u32,
        writes: Arc<Mutex<Vec<Duration>>>,
    }
    impl Link for Observe {
        fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
            Ok(Box::new(ObservedPort {
                inner: self.inner.open(port, baud_rate)?,
                writes: Arc::clone(&self.writes),
            }))
        }

        fn candidates(&self) -> Vec<Candidate> {
            self.inner.candidates()
        }

        fn time_scale(&self) -> u32 {
            self.scale
        }
    }
    struct ObservedPort {
        inner: Box<dyn Port>,
        writes: Arc<Mutex<Vec<Duration>>>,
    }
    impl Port for ObservedPort {
        fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
            self.writes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(timeout);
            self.inner.write_all(bytes, timeout)
        }

        fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
            self.inner.read(buffer, timeout)
        }
    }
    for (scale, expected) in [
        (1, [Duration::from_secs(3), Duration::from_secs(600)]),
        (
            1000,
            [Duration::from_millis(100), Duration::from_millis(600)],
        ),
    ] {
        let sim = Simulator::start();
        let writes = Arc::new(Mutex::new(vec![]));
        let link = Observe {
            inner: sim.link(),
            scale,
            writes: Arc::clone(&writes),
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
            session
                .command("^u", Duration::from_secs(600))
                .unwrap()
                .body,
            "1#UWarsztat Łódź"
        );
        assert_eq!(*writes.lock().unwrap(), expected);
    }
}
