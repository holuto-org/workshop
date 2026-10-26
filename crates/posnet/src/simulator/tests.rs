use super::{Simulator, UNIQUE_NUMBER};
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
        Some(UNIQUE_NUMBER)
    );
}
