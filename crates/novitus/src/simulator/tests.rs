use crate::{session::Session, simulator::Simulator};
use fiscal::{error::Error, link::Connection};
use std::{io::ErrorKind, time::Duration};

#[test]
fn fragmented_queries_preserve_polish_header_endpoint_and_configuration() {
    let sim = Simulator::start();
    sim.set_fragment_size(1);
    sim.set_endpoint("PC2");
    sim.set_dialect("0");
    let link = sim.link();
    let connection = Connection {
        port: sim.address().to_owned(),
        baud_rate: 9600,
    };
    let mut session = Session::open(link.as_ref(), &connection).unwrap();
    assert_eq!(
        session
            .command("50$i", Duration::from_secs(3))
            .unwrap()
            .body,
        "50$IPC2\r"
    );
    assert_eq!(
        session
            .command("0;1^c5/5/", Duration::from_secs(3))
            .unwrap()
            .body,
        "1^c5\r0\r"
    );
    assert_eq!(
        session.command("^u", Duration::from_secs(3)).unwrap().body,
        "1#UWarsztat Łódź"
    );
    assert_eq!(
        session
            .command("23#s", Duration::from_secs(3))
            .unwrap()
            .body,
        concat!(
            "2#X0;1;0;1;1;0;26;10;24/23.00/8.00/5.00/0.00/98.99/99.99/99.99/",
            "0/0/0/0/0/0/0/0/0/ABC1234567890"
        )
    );
}

#[test]
fn link_enforces_exclusive_sessions_and_can_reopen_after_drop() {
    let sim = Simulator::start();
    let connection = Connection {
        port: sim.address().to_owned(),
        baud_rate: 9600,
    };
    let link = sim.link();
    let session = Session::open(link.as_ref(), &connection).unwrap();
    assert!(matches!(
        Session::open(sim.link().as_ref(), &connection),
        Err(Error::Open { source, .. }) if source.kind() == ErrorKind::PermissionDenied
    ));
    drop(session);
    let mut session = Session::open(link.as_ref(), &connection).unwrap();
    assert_eq!(
        session
            .command("50$i", Duration::from_secs(3))
            .unwrap()
            .body,
        "50$IPC1\r"
    );
}

#[test]
fn mismatched_close_is_refused_without_completing_or_discarding_the_receipt() {
    let sim = Simulator::start();
    let connection = Connection {
        port: sim.address().to_owned(),
        baud_rate: 9600,
    };
    let mut session = Session::open(sim.link().as_ref(), &connection).unwrap();
    for body in ["0$h", "1$lNaprawa\r1\rA/10/10/", "1;0$b10/\r"] {
        let _reply = session.command(body, Duration::from_secs(3)).unwrap();
    }
    assert!(matches!(
        session.command("0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r9/0/0/9/0/", Duration::from_secs(3)),
        Err(Error::Device { code: 27, command, .. }) if command == "$y"
    ));
    assert_eq!(sim.receipts(), 0);
    assert!(matches!(
        session.command("1;26;10;24#r", Duration::from_secs(3)),
        Err(Error::Device { code: 21, .. })
    ));
    assert_eq!(sim.reports(), 0);
    for body in [
        "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r10/0/0/10/0/",
        "12$z",
        "28;0$z",
    ] {
        let _reply = session.command(body, Duration::from_secs(3)).unwrap();
    }
    assert_eq!(sim.receipts(), 1);
}
