use super::Simulators;
use agent::settings::Settings;
use fiscal::device::Vendor;
use std::io::ErrorKind;

#[test]
fn selection_replaces_both_stale_port_and_vendor_without_changing_credentials() {
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
