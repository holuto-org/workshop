use super::*;
use crate::simulator::{Fault, Simulator};

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
fn realtime_status_bits_decode_paper_and_cover() {
    let dle = DleStatus::decode(0x77).unwrap();
    assert!(dle.online && dle.paper_or_battery && dle.mechanism_error);
    let enq = EnqStatus::decode(0x6F).unwrap();
    assert!(enq.fiscal && enq.command_ok && enq.transaction && enq.finalized);
    let byte4 = MechanismStatus::from_documented_byte4(0x15);
    assert!(byte4.paper_out && byte4.cover_open && byte4.waiting_for_user);
    assert!(!byte4.mechanism_error);
    assert!(DleStatus::decode(0x60).is_err());
    assert!(EnqStatus::decode(0x70).is_err());
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
            last_print: 0,
            last_receipt: 0,
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
            .command("30#v", Duration::from_secs(3))
            .unwrap()
            .body,
        "1#RNOVITUS HD ONLINE/1.60"
    );
}

#[test]
fn bad_status_and_corrupt_reply_fail_closed() {
    assert!(ExtendedStatus::parse("@s0;0;0;00000000;").is_ok());
    for invalid in [
        "@s0;0;0;0000000;",
        "@s0;0;0;GG000000;",
        "@s0;0;0;00000000",
        "@s0;0;0;00000000;1;",
    ] {
        assert!(ExtendedStatus::parse(invalid).is_err(), "{invalid}");
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
    assert!(session.command("30#v", Duration::from_secs(3)).is_ok());
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
    for fault in [Fault::Disconnect, Fault::Reply(b"\x1bP1#U".to_vec())] {
        let sim = Simulator::start();
        sim.fault("^u", fault);
        let mut session = open(&sim);
        assert!(matches!(
            session.command("^u", Duration::from_secs(3)),
            Err(Error::Io(_) | Error::Timeout("Novitus"))
        ));
        assert_eq!(sim.frames().len(), 2);
        assert!(matches!(
            session.command("^u", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
    }
}

#[test]
fn unscaled_links_keep_the_callers_exact_deadline() {
    let sim = Simulator::start();
    let mut session = open(&sim);
    session.time_scale = 1;
    assert_eq!(
        session.timeout(Duration::from_millis(1)),
        Duration::from_millis(1)
    );
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
