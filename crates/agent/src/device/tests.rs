use super::detect;
use crate::testing::printers::{RoutingLink, Simulator, VENDORS};
use elzab::frame as elzab_frame;
use fiscal::{
    device::Vendor,
    error::Error,
    link::{Candidate, Connection, Link, Port},
};
use novitus::{frame as novitus_frame, simulator::Simulator as NovitusSimulator};
use std::{io, sync::Arc};

fn connection(port: &str) -> Connection {
    Connection {
        port: port.to_owned(),
        baud_rate: 9600,
    }
}

#[test]
fn remembered_vendor_probes_only_that_brand() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        let link = RoutingLink::new(vec![(
            Candidate {
                port: sim.address().to_owned(),
                hint: Some(Vendor::Posnet),
            },
            sim.link(),
        )]);
        let (driver, device) = detect(
            Arc::<RoutingLink>::clone(&link),
            connection(sim.address()),
            Some(vendor),
            true,
        )
        .unwrap();
        assert_eq!(driver.vendor(), vendor);
        assert_eq!(device.vendor, vendor);
        assert_eq!(*link.opened.lock().unwrap(), [sim.address()]);
        let first = sim.frames().into_iter().next().unwrap();
        match vendor {
            Vendor::Posnet => assert!(first.starts_with(b"\x02getrealid\t")),
            Vendor::Elzab => assert_eq!(elzab_frame::decode(&first).unwrap().1, b"XTR"),
            Vendor::Novitus => assert_eq!(novitus_frame::unframe(&first).unwrap(), b"30#v"),
        }
    }
}

#[test]
fn hinted_port_probes_only_the_hinted_brand() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        let hint = match vendor {
            Vendor::Posnet => Vendor::Elzab,
            Vendor::Elzab | Vendor::Novitus => Vendor::Posnet,
        };
        let link = RoutingLink::new(vec![(
            Candidate {
                port: sim.address().to_owned(),
                hint: Some(hint),
            },
            sim.link(),
        )]);
        assert!(
            detect(
                Arc::<RoutingLink>::clone(&link),
                connection("missing"),
                None,
                true
            )
            .is_err()
        );
        assert_eq!(*link.opened.lock().unwrap(), ["missing", sim.address()]);
        assert_eq!(sim.receipts(), 0);
        assert_eq!(sim.reports(), 0);
    }
}

#[test]
fn unhinted_probe_order_is_inert_on_every_other_brand() {
    for (vendor, attempts) in [
        (Vendor::Posnet, 1),
        (Vendor::Elzab, 2),
        (Vendor::Novitus, 3),
    ] {
        let sim = Simulator::start(vendor);
        let link = RoutingLink::new(vec![(
            Candidate {
                port: sim.address().to_owned(),
                hint: None,
            },
            sim.link(),
        )]);
        let (driver, device) = detect(
            Arc::<RoutingLink>::clone(&link),
            connection(sim.address()),
            None,
            true,
        )
        .unwrap();
        assert_eq!(driver.vendor(), vendor);
        assert!(device.ready(), "{vendor:?}: {:?}", device.problems);
        assert_eq!(link.opened.lock().unwrap().len(), attempts);
        let frames = sim.frames();
        assert!(frames.first().unwrap().starts_with(b"\x02getrealid\t"));
        if vendor == Vendor::Novitus {
            let [_, elzab_probe, novitus_probe, ..] = frames.as_slice() else {
                panic!("Novitus must receive both foreign probes before its identity query");
            };
            assert_eq!(elzab_frame::decode(elzab_probe).unwrap().1, b"XTR");
            assert_eq!(novitus_frame::unframe(novitus_probe).unwrap(), b"30#v");
        }
        assert_eq!(sim.receipts(), 0);
        assert_eq!(sim.reports(), 0);
        assert_eq!(sim.fiscal_frames(), Vec::<Vec<u8>>::new());
    }
}

#[test]
fn no_scan_while_an_operation_blocks() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        let link = RoutingLink::new(vec![(
            Candidate {
                port: sim.address().to_owned(),
                hint: Some(vendor),
            },
            sim.link(),
        )]);
        assert!(
            detect(
                Arc::<RoutingLink>::clone(&link),
                connection("unavailable"),
                Some(vendor),
                false
            )
            .is_err()
        );
        assert_eq!(*link.opened.lock().unwrap(), ["unavailable"]);
        assert_eq!(link.enumeration_count(), 0);
        assert_eq!(sim.frames(), Vec::<Vec<u8>>::new());
    }
}

#[test]
fn stale_vendor_cache_rediscovers_without_repeating_the_failed_brand() {
    let sim = Simulator::start(Vendor::Novitus);
    let link = RoutingLink::new(vec![(
        Candidate {
            port: sim.address().to_owned(),
            hint: None,
        },
        sim.link(),
    )]);
    let (driver, _) = detect(
        Arc::<RoutingLink>::clone(&link),
        connection(sim.address()),
        Some(Vendor::Posnet),
        true,
    )
    .unwrap();
    assert_eq!(driver.vendor(), Vendor::Novitus);
    assert_eq!(link.opened.lock().unwrap().len(), 3);
}

#[test]
fn mixed_candidates_respect_hints_and_skip_foreign_devices() {
    let foreign = Simulator::start(Vendor::Novitus);
    if let Simulator::Novitus(sim) = &foreign {
        sim.set_identity("OTHER BRAND", "1.60");
    }
    let elzab = Simulator::start(Vendor::Elzab);
    let posnet = Simulator::start(Vendor::Posnet);
    let link = RoutingLink::new(vec![
        (
            Candidate {
                port: foreign.address().to_owned(),
                hint: None,
            },
            foreign.link(),
        ),
        (
            Candidate {
                port: elzab.address().to_owned(),
                hint: Some(Vendor::Elzab),
            },
            elzab.link(),
        ),
        (
            Candidate {
                port: posnet.address().to_owned(),
                hint: Some(Vendor::Posnet),
            },
            posnet.link(),
        ),
    ]);
    let (driver, device) = detect(
        Arc::<RoutingLink>::clone(&link),
        connection("missing"),
        None,
        true,
    )
    .unwrap();
    assert_eq!(driver.connection().port, elzab.address());
    assert_eq!(device.vendor, Vendor::Elzab);
    assert_eq!(
        *link.opened.lock().unwrap(),
        [
            "missing",
            foreign.address(),
            foreign.address(),
            foreign.address(),
            elzab.address()
        ]
    );
    assert_eq!(posnet.frames(), Vec::<Vec<u8>>::new());
}

#[test]
fn unsupported_elzab_model_is_reported_instead_of_a_later_silent_port() {
    let elzab = Simulator::start(Vendor::Elzab);
    if let Simulator::Elzab(sim) = &elzab {
        sim.set_identity("ELZAB Zeta Online", "MD06.02");
    }
    let foreign = Simulator::start(Vendor::Novitus);
    if let Simulator::Novitus(sim) = &foreign {
        sim.set_identity("OTHER BRAND", "1.60");
    }
    let link = RoutingLink::new(vec![
        (
            Candidate {
                port: elzab.address().to_owned(),
                hint: None,
            },
            elzab.link(),
        ),
        (
            Candidate {
                port: foreign.address().to_owned(),
                hint: None,
            },
            foreign.link(),
        ),
    ]);
    let error = detect(
        Arc::<RoutingLink>::clone(&link),
        connection("missing"),
        None,
        true,
    )
    .err()
    .unwrap();
    assert!(
        matches!(&error, Error::Unsupported(message) if message.contains("ELZAB Zeta Online")),
        "{error:?}"
    );
    // Novitus is never probed on a port where Elzab already answered.
    assert_eq!(
        *link.opened.lock().unwrap(),
        [
            "missing",
            elzab.address(),
            elzab.address(),
            foreign.address(),
            foreign.address(),
            foreign.address()
        ]
    );
    assert_eq!(elzab.fiscal_frames(), Vec::<Vec<u8>>::new());
}

#[test]
fn remembered_elzab_with_unsupported_model_gets_no_foreign_probe() {
    let elzab = Simulator::start(Vendor::Elzab);
    if let Simulator::Elzab(sim) = &elzab {
        sim.set_identity("ELZAB Zeta Online", "MD06.02");
    }
    let link = RoutingLink::new(vec![(
        Candidate {
            port: elzab.address().to_owned(),
            hint: None,
        },
        elzab.link(),
    )]);
    let error = detect(
        Arc::<RoutingLink>::clone(&link),
        connection(elzab.address()),
        Some(Vendor::Elzab),
        true,
    )
    .err()
    .unwrap();
    assert!(matches!(error, Error::Unsupported(_)), "{error:?}");
    assert_eq!(*link.opened.lock().unwrap(), [elzab.address()]);
    let frames = elzab.frames();
    let [only] = frames.as_slice() else {
        panic!("only the identity query may reach the port: {frames:?}");
    };
    assert_eq!(elzab_frame::decode(only).unwrap().1, b"XTR");
}

#[test]
fn automatic_port_without_any_printer_reports_no_printer_without_opening_a_port() {
    let link = RoutingLink::new(Vec::new());
    let error = detect(Arc::<RoutingLink>::clone(&link), connection(""), None, true)
        .err()
        .unwrap();
    assert!(matches!(error, Error::NoPort), "{error:?}");
    assert_eq!(*link.opened.lock().unwrap(), Vec::<String>::new());
}

#[derive(Debug)]
struct Denied;
impl Link for Denied {
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Err(Error::Open {
            port: port.to_owned(),
            source: if port == "denied" {
                io::ErrorKind::PermissionDenied
            } else {
                io::ErrorKind::NotFound
            }
            .into(),
        })
    }

    fn candidates(&self) -> Vec<Candidate> {
        vec![Candidate {
            port: "denied".to_owned(),
            hint: Some(Vendor::Elzab),
        }]
    }
}
#[test]
fn permission_denied_candidate_retains_the_operator_diagnostic() {
    let error = detect(Arc::new(Denied), connection("missing"), None, true)
        .err()
        .unwrap();
    assert!(matches!(&error, Error::Invalid(message) if message ==
        "Brak uprawnień do portu denied. Na Linuksie dodaj użytkownika do grupy dialout i \
         zaloguj się ponownie."));
}

#[test]
fn matching_hint_selects_each_vendor_without_foreign_queries() {
    for vendor in VENDORS {
        let sim = Simulator::start(vendor);
        let link = RoutingLink::new(vec![(
            Candidate {
                port: sim.address().to_owned(),
                hint: Some(vendor),
            },
            sim.link(),
        )]);
        let (driver, device) = detect(
            Arc::<RoutingLink>::clone(&link),
            connection(sim.address()),
            None,
            true,
        )
        .unwrap();
        assert_eq!(driver.vendor(), vendor);
        assert_eq!(device.vendor, vendor);
        assert_eq!(*link.opened.lock().unwrap(), [sim.address()]);
    }
}

#[test]
fn blocked_detection_never_falls_back_to_another_brand_on_the_same_port() {
    let sim = Simulator::start(Vendor::Elzab);
    let link = RoutingLink::new(vec![(
        Candidate {
            port: sim.address().to_owned(),
            hint: Some(Vendor::Elzab),
        },
        sim.link(),
    )]);
    assert!(
        detect(
            Arc::<RoutingLink>::clone(&link),
            connection(sim.address()),
            Some(Vendor::Posnet),
            false
        )
        .is_err()
    );
    assert_eq!(link.enumeration_count(), 0);
    assert_eq!(*link.opened.lock().unwrap(), [sim.address()]);
    assert_eq!(sim.frames().len(), 1);
    assert!(
        sim.frames()
            .first()
            .unwrap()
            .starts_with(b"\x02getrealid\t")
    );
    if let Simulator::Elzab(peer) = &sim {
        assert!(!peer.transaction_open());
        assert!(!peer.daily_pending());
    }
}

#[test]
fn novitus_pc2_is_not_adopted_and_a_non_native_dialect_is_unready() {
    let sim = NovitusSimulator::start();
    sim.set_identity("NOVITUS HD II ONLINE", "3.50");
    sim.set_endpoint("PC2");
    assert!(
        detect(
            sim.link(),
            connection(sim.address()),
            Some(Vendor::Novitus),
            false
        )
        .is_err()
    );
    assert_eq!(
        sim.frames()
            .iter()
            .map(|frame| novitus_frame::unframe(frame).unwrap())
            .collect::<Vec<_>>(),
        [b"30#v".as_slice(), b"50$i", b"#c"]
    );
    sim.set_endpoint("PC1");
    sim.set_dialect("0");
    let (_, device) = detect(
        sim.link(),
        connection(sim.address()),
        Some(Vendor::Novitus),
        false,
    )
    .unwrap();
    assert!(!device.ready());
    assert_eq!(
        device.description(),
        "Wymagany jest natywny protokół NOVITUS"
    );
    assert_eq!(sim.receipts(), 0);
    assert_eq!(sim.reports(), 0);
}
