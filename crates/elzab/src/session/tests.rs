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
fn transport_rejection_must_be_single_frame() {
    let sim = Simulator::start();
    sim.fault(
        "LOR",
        Fault::Reply(frame::encode("1\x1dSL\x1d2\x1d2", b"").unwrap()),
    );
    assert!(matches!(
        open(&sim).command("LOR", "LOR\x1dC0", Duration::from_secs(3)),
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
            .command("FNR", "FNR", Duration::from_secs(3))
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
        assert!(
            session
                .command("XTR", "XTR", Duration::from_secs(3))
                .is_err()
        );
        assert!(matches!(
            session.command("FNR", "FNR", Duration::from_secs(3)),
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
            open(&sim).command("LOR", "LOR\x1dC0", Duration::from_secs(3)),
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
            open(&sim).command("LOR", "LOR\x1dC0", Duration::from_secs(3)),
            Err(Error::Protocol(_))
        ));
    }
}
