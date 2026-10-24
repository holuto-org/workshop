use std::time::{Duration, Instant};

use super::{Fault, SERIAL, Simulator};
use fiscal::{
    error::Error,
    link::{Connection, DEFAULT_TIMEOUT},
};

use crate::session::Session;

fn connect(sim: &Simulator) -> Session {
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
fn the_clock_is_settable_and_preserves_the_offset_in_the_repeated_hour() {
    let sim = Simulator::start();
    sim.set_clock("2026-10-25T01:30:00Z".parse().unwrap());
    let reply = connect(&sim)
        .command("rtcget", &[], DEFAULT_TIMEOUT)
        .unwrap();
    assert_eq!(reply.get("da"), Some("2026-10-25;02:30"));
    assert_eq!(reply.get("tm"), Some("2026-10-25T02:30:00+01:00"));
}

#[test]
fn a_held_command_still_answers_asynchronous_status_queries() {
    let sim = Simulator::start();
    sim.set_status("ds", "2");
    sim.set_status("pr", "1");
    let hold = sim.hold("dailyrep");
    let mut session = connect(&sim);
    let _deadline = session
        .send(
            "dailyrep",
            &[("da", "2026-01-01".to_owned())],
            DEFAULT_TIMEOUT,
        )
        .unwrap();
    assert!(hold.reached(Duration::from_secs(1)));
    let device = session.command("!sdev", &[], DEFAULT_TIMEOUT).unwrap();
    assert_eq!(device.get("ds"), Some("2"));
    assert_eq!(
        session
            .command("!sprn", &[], DEFAULT_TIMEOUT)
            .unwrap()
            .get("pr"),
        Some("1")
    );
    hold.release();
    assert!(
        session
            .receive("dailyrep", Instant::now() + Duration::from_secs(1))
            .is_ok()
    );
    assert_eq!(sim.reports()[0].command, "dailyrep");
}

#[test]
fn a_second_session_is_refused_until_the_first_is_dropped() {
    let sim = Simulator::start();
    let first = connect(&sim);
    assert!(matches!(
        Session::open(
            sim.link().as_ref(),
            &Connection {
                port: sim.address().to_owned(),
                baud_rate: 9600
            }
        ),
        Err(Error::Open { .. })
    ));
    drop(first);
    assert_eq!(
        connect(&sim)
            .command("getrealid", &[], DEFAULT_TIMEOUT)
            .unwrap()
            .get("nu"),
        Some(SERIAL)
    );
}

#[test]
fn execution_faults_change_the_device_before_the_reply_is_lost() {
    for fault in [Fault::LoseReply, Fault::CorruptReply, Fault::Disconnect] {
        let sim = Simulator::start();
        sim.fault("dailyrep", fault);
        assert!(
            connect(&sim)
                .command(
                    "dailyrep",
                    &[("da", "2026-01-01".to_owned())],
                    DEFAULT_TIMEOUT
                )
                .is_err()
        );
        assert_eq!(sim.reports()[0].command, "dailyrep");
        assert_eq!(sim.frames().len(), 1);
    }
}
