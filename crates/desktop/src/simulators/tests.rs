use super::{SAMPLE_JOBS, Simulators};
use agent::{
    settings::Settings,
    store::StateDir,
    worker::{Event, Worker},
};
use fiscal::device::Vendor;
use std::{
    io::ErrorKind,
    sync::{Arc, mpsc},
    time::Duration,
};
use tempfile::TempDir;

#[test]
fn selection_points_at_the_chosen_printer_and_the_sample_holuto() {
    let peers = Simulators::start();
    for (choice, vendor, address) in [
        ("posnet", Vendor::Posnet, peers.posnet.address()),
        ("novitus", Vendor::Novitus, peers.novitus.address()),
        ("elzab", Vendor::Elzab, peers.elzab.address()),
    ] {
        let settings = Settings {
            port: "old-port".into(),
            vendor: Some(Vendor::Posnet),
            token: "synthetic-token".into(),
            ..Settings::default()
        };
        let (selected, link) = peers.connection(settings.clone(), choice).unwrap();
        assert_eq!(
            selected,
            Settings {
                port: address.to_owned(),
                vendor: Some(vendor),
                api_url: peers.holuto.url().to_owned(),
                token: peers.holuto.token().into(),
                start_on_launch: true,
                ..settings
            }
        );
        assert_eq!(
            link.candidates()
                .iter()
                .map(|candidate| candidate.port.as_str())
                .collect::<Vec<_>>(),
            [address]
        );
    }
}

#[test]
fn invalid_selector_is_refused_instead_of_silently_using_posnet() {
    let peers = Simulators::start();
    let error = peers
        .connection(Settings::default(), "unknown")
        .err()
        .unwrap();
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "WORKSHOP_SIMULATOR: oczekiwano posnet, novitus lub elzab."
    );
}

#[test]
fn every_simulated_printer_prints_the_queued_sale_and_advance_once() {
    for choice in ["posnet", "novitus", "elzab"] {
        let peers = Simulators::start();
        let temporary = TempDir::new().unwrap();
        let dir = Arc::new(StateDir::open(temporary.path().to_owned()).unwrap());
        let (settings, link) = peers.connection(Settings::default(), choice).unwrap();
        let (events, received) = mpsc::channel();
        let worker = Worker::spawn(dir, settings, link, move |event| {
            let _sent = events.send(event);
        })
        .unwrap();
        let settled = || {
            SAMPLE_JOBS.iter().all(|id| {
                let state = &peers.holuto.job(*id)["state"];
                state != "queued" && state != "taken"
            })
        };
        let mut problem = None;
        // Each poll ends with the device; bounded only to fail instead of hanging.
        while !settled() {
            match received.recv_timeout(Duration::from_secs(10)) {
                Ok(Event::Problem(shown)) => problem = shown,
                Ok(_) => {},
                Err(error) => panic!("{choice}: samples never settled ({error}): {problem:?}"),
            }
        }
        worker.stop();
        loop {
            match received.recv_timeout(Duration::from_secs(10)) {
                Ok(Event::Stopped) => break,
                Ok(_) => {},
                Err(error) => panic!("{choice}: worker never stopped: {error}"),
            }
        }
        worker.join();
        for id in SAMPLE_JOBS {
            let job = peers.holuto.job(id);
            assert_eq!(job["state"], "done", "{choice} prints sample {id}: {job}");
            assert!(job["receiptNumber"].is_string(), "{choice}: {job}");
        }
    }
}
