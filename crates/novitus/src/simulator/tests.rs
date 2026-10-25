use crate::{session::Session, simulator::Simulator};
use fiscal::link::Connection;
use std::time::Duration;

#[test]
fn fragmented_queries_preserve_polish_header_endpoint_and_candidates() {
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
    assert!(Session::open(link.as_ref(), &connection).is_err());
}
