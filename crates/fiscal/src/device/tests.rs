use super::{Capabilities, Device, Problem, VatKind, VatRate, Vendor};
use crate::error::Error;

fn device() -> Device {
    Device {
        vendor: Vendor::Posnet,
        model: "Test".to_owned(),
        firmware: "1".to_owned(),
        unique_number: "TST0000000001".to_owned(),
        problems: vec![],
        vat: vec![],
        capabilities: Capabilities {
            max_item_name: 64,
            max_lines: 255,
            max_company_name: 42,
            max_buyer_nip: 30,
            max_quantity_decimals: 5,
            allowed_characters: " Aą1".to_owned(),
            advance_deductions: false,
            zero_total_receipts: false,
            company_footer: false,
        },
    }
}

#[test]
fn persisted_vendor_names_keep_their_exact_meaning() {
    for (vendor, name) in [
        (Vendor::Posnet, "\"posnet\""),
        (Vendor::Novitus, "\"novitus\""),
        (Vendor::Elzab, "\"elzab\""),
    ] {
        assert_eq!(serde_json::to_string(&vendor).unwrap(), name);
        assert_eq!(serde_json::from_str::<Vendor>(name).unwrap(), vendor);
    }
}

#[test]
fn vat_rates_label_slots_and_report_activity() {
    for (kind, label, active) in [
        (VatKind::Rate { basis_points: 2300 }, "A · 23%", true),
        (VatKind::Rate { basis_points: 850 }, "A · 8,5%", true),
        (VatKind::Rate { basis_points: 801 }, "A · 8,01%", true),
        (VatKind::Rate { basis_points: 0 }, "A · 0%", true),
        (VatKind::Exempt, "A · zwolniona", true),
        (VatKind::Inactive, "A · nieaktywna", false),
    ] {
        let rate = VatRate { slot: 0, kind };
        assert_eq!(rate.label(), label);
        assert_eq!(rate.active(), active, "{label}");
    }
    assert_eq!(
        VatRate {
            slot: 6,
            kind: VatKind::Exempt
        }
        .label(),
        "G · zwolniona"
    );
}

#[test]
fn readiness_tolerates_only_the_problems_allowed_for_each_operation() {
    let read_only = |periodic_reports| Problem::ReadOnly { periodic_reports };
    for (problems, ready, daily, periodic, resolvable, description) in [
        (vec![], true, true, true, true, "Gotowa do drukowania"),
        (
            vec![Problem::DailyReportOverdue],
            false,
            true,
            false,
            true,
            "Wymagany zaległy raport dobowy",
        ),
        (
            vec![read_only(true)],
            false,
            false,
            true,
            true,
            "Drukarka jest w trybie tylko do odczytu",
        ),
        (
            vec![read_only(false)],
            false,
            false,
            false,
            true,
            "Drukarka jest w trybie tylko do odczytu",
        ),
        (
            vec![Problem::DailyReportOverdue, read_only(true)],
            false,
            false,
            false,
            true,
            "Wymagany zaległy raport dobowy. Drukarka jest w trybie tylko do odczytu",
        ),
        (
            vec!["Brak papieru".into()],
            false,
            false,
            false,
            false,
            "Brak papieru",
        ),
        (
            vec![Problem::DailyReportOverdue, "Brak papieru".into()],
            false,
            false,
            false,
            false,
            "Wymagany zaległy raport dobowy. Brak papieru",
        ),
        (
            vec![read_only(true), "Brak papieru".into()],
            false,
            false,
            false,
            false,
            "Drukarka jest w trybie tylko do odczytu. Brak papieru",
        ),
        (
            vec![read_only(false), "Brak papieru".into()],
            false,
            false,
            false,
            false,
            "Drukarka jest w trybie tylko do odczytu. Brak papieru",
        ),
    ] {
        let device = Device {
            problems,
            ..device()
        };
        assert_eq!(device.ready(), ready, "{description}");
        assert_eq!(device.daily_report_allowed(), daily, "{description}");
        assert_eq!(device.periodic_reports_allowed(), periodic, "{description}");
        assert_eq!(device.description(), description);
        for (result, allowed) in [
            (device.require_ready(), ready),
            (device.require_periodic_reports(), periodic),
            (device.require_resolvable(), resolvable),
        ] {
            match (result, allowed) {
                (Ok(()), true) => {},
                (Err(Error::NotReady(message)), false) => {
                    assert_eq!(message.to_string(), description);
                },
                (result, _) => panic!("{description}: {result:?}"),
            }
        }
    }
}

#[test]
fn invalid_identity_blocks_every_operation_even_when_problems_are_tolerated() {
    for identity in [
        "",
        "ABC123456789",
        "ABC12345678901",
        "abc1234567890",
        "AB11234567890",
        "ABC123456789x",
        "ĄBC1234567890",
        " ABC1234567890",
        "ABC1234567890 ",
    ] {
        let device = Device {
            unique_number: identity.to_owned(),
            problems: vec![
                Problem::DailyReportOverdue,
                Problem::ReadOnly {
                    periodic_reports: true,
                },
            ],
            vat: vec![VatRate {
                slot: 0,
                kind: VatKind::Exempt,
            }],
            ..device()
        };
        assert!(!device.ready(), "{identity:?}");
        assert!(!device.daily_report_allowed(), "{identity:?}");
        assert!(!device.periodic_reports_allowed(), "{identity:?}");
        assert!(!device.describable(), "{identity:?}");
        for result in [
            device.require_ready(),
            device.require_periodic_reports(),
            device.require_resolvable(),
        ] {
            match result {
                Err(Error::NotReady(message)) => assert_eq!(
                    message.to_string(),
                    "Nieprawidłowy numer unikatowy drukarki",
                    "{identity:?}"
                ),
                result => panic!("{identity:?}: {result:?}"),
            }
        }
    }
    for identity in ["ABC1234567890", "ZZZ0000000000"] {
        let device = Device {
            unique_number: identity.to_owned(),
            ..device()
        };
        assert!(device.require_ready().is_ok());
        assert!(device.describable());
    }
}

#[test]
fn vat_assignments_gate_readiness_reports_resolution_and_telemetry() {
    for (slot, kind, legal) in [
        (0, VatKind::Rate { basis_points: 2300 }, true),
        (1, VatKind::Rate { basis_points: 800 }, true),
        (2, VatKind::Rate { basis_points: 500 }, true),
        (3, VatKind::Rate { basis_points: 0 }, true),
        (4, VatKind::Exempt, true),
        (0, VatKind::Rate { basis_points: 2299 }, false),
        (1, VatKind::Rate { basis_points: 801 }, false),
        (2, VatKind::Rate { basis_points: 501 }, false),
        (3, VatKind::Rate { basis_points: 1 }, false),
        (4, VatKind::Rate { basis_points: 0 }, false),
        (0, VatKind::Exempt, false),
        (5, VatKind::Rate { basis_points: 1900 }, true),
        (6, VatKind::Exempt, true),
        (7, VatKind::Rate { basis_points: 2300 }, false),
        (7, VatKind::Inactive, false),
        (255, VatKind::Inactive, false),
    ]
    .into_iter()
    .chain((0..7).map(|slot| (slot, VatKind::Inactive, true)))
    {
        let device = Device {
            vat: vec![VatRate { slot, kind }],
            ..device()
        };
        let label = format!("{slot}: {kind:?}");
        assert_eq!(device.ready(), legal, "{label}");
        assert_eq!(device.daily_report_allowed(), legal, "{label}");
        assert_eq!(device.periodic_reports_allowed(), legal, "{label}");
        assert_eq!(device.describable(), legal, "{label}");
        for result in [
            device.require_ready(),
            device.require_periodic_reports(),
            device.require_resolvable(),
        ] {
            match (result, legal) {
                (Ok(()), true) => {},
                (Err(Error::NotReady(message)), false) => {
                    assert_eq!(
                        message.to_string(),
                        "Nieprawidłowe przypisanie liter VAT A–E"
                    );
                },
                (result, _) => panic!("{label}: {result:?}"),
            }
        }
    }
}

#[test]
fn unknown_capabilities_are_not_advertised_but_readiness_problems_can_be() {
    let known = device();
    assert!(known.describable());
    let paper_out = Device {
        problems: vec!["Brak papieru".into()],
        ..device()
    };
    assert!(paper_out.describable());
    for caps in [
        Capabilities {
            max_item_name: 0,
            ..known.capabilities.clone()
        },
        Capabilities {
            max_lines: 0,
            ..known.capabilities.clone()
        },
        Capabilities {
            max_company_name: 0,
            ..known.capabilities.clone()
        },
        Capabilities {
            max_quantity_decimals: 9,
            ..known.capabilities.clone()
        },
        Capabilities {
            allowed_characters: String::new(),
            ..known.capabilities
        },
    ] {
        let device = Device {
            capabilities: caps,
            ..device()
        };
        assert!(!device.describable(), "{:?}", device.capabilities);
    }
    for decimals in [0, 8] {
        let mut device = device();
        device.capabilities.max_quantity_decimals = decimals;
        assert!(device.describable(), "{decimals}");
    }
}

#[test]
fn operation_binding_compares_the_complete_unique_number_without_normalizing() {
    let device = device();
    assert!(device.require_unique_number("TST0000000001").is_ok());
    for other in ["TST0000000002", "tst0000000001", "TST 0000000001", ""] {
        let result = device.require_unique_number(other);
        assert!(matches!(result, Err(Error::Invalid(_))));
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Podłączona drukarka TST0000000001 nie jest drukarką {other}, \
                 której dotyczy operacja."
            )
        );
        assert_eq!(device.unique_number, "TST0000000001");
    }
}

#[test]
fn existing_readiness_and_uncertainty_messages_render_in_each_ui_language() {
    use i18n::Language;

    let mut device = device();
    device.problems.push(Problem::DailyReportOverdue);
    let error = Error::Uncertain(Box::new(device.require_ready().unwrap_err()));
    for (language, readiness, warning) in [
        (
            Language::En,
            "Overdue daily report required",
            "Do not send it again",
        ),
        (
            Language::Pl,
            "Wymagany zaległy raport dobowy",
            "Nie wysyłaj jej ponownie",
        ),
        (
            Language::Es,
            "Se requiere el informe diario pendiente",
            "No vuelva a enviarla",
        ),
        (
            Language::Ru,
            "Требуется просроченный суточный отчёт",
            "Не отправляйте её повторно",
        ),
    ] {
        let text = error.message().render(language);
        assert!(text.contains(readiness), "{text}");
        assert!(text.contains(warning), "{text}");
    }
    assert_eq!(
        error.to_string(),
        concat!(
            "Wymagany zaległy raport dobowy\nWynik operacji wymaga sprawdzenia. ",
            "Nie wysyłaj jej ponownie przed sprawdzeniem drukarki."
        )
    );
}
