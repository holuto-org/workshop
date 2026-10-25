use super::{Capabilities, Device, Problem, VatKind, VatRate, Vendor, valid_unique_number};

#[test]
fn vendor_serializes_snake_case() {
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
fn daily_report_eligibility_only_ignores_overdue_reports_for_every_vendor() {
    for vendor in [Vendor::Posnet, Vendor::Novitus, Vendor::Elzab] {
        for (problems, ready, daily) in [
            (vec![], true, true),
            (vec![Problem::DailyReportOverdue], false, true),
            (vec!["Brak papieru".into()], false, false),
            (
                vec![Problem::DailyReportOverdue, "Brak papieru".into()],
                false,
                false,
            ),
        ] {
            let device = Device {
                vendor,
                model: String::new(),
                firmware: String::new(),
                unique_number: "TST0000000001".to_owned(),
                problems,
                vat: vec![],
                capabilities: Capabilities {
                    max_item_name: 0,
                    max_lines: 0,
                    max_company_name: 0,
                    max_buyer_nip: 0,
                    max_quantity_decimals: 0,
                    allowed_characters: String::new(),
                    advance_deductions: false,
                    zero_total_receipts: false,
                    company_footer: false,
                },
            };
            assert_eq!(device.ready(), ready, "{vendor:?}: {:?}", device.problems);
            assert_eq!(
                device.daily_report_allowed(),
                daily,
                "{vendor:?}: {:?}",
                device.problems
            );
            assert_eq!(device.require_ready().is_ok(), ready);
        }
    }
}

#[test]
fn read_only_mode_allows_reports_only_when_permitted_and_always_allows_resolution() {
    let reporting = Problem::ReadOnly {
        periodic_reports: true,
    };
    let silent = Problem::ReadOnly {
        periodic_reports: false,
    };
    for (problems, ready, daily, periodic, resolvable) in [
        (vec![], true, true, true, true),
        (vec![reporting.clone()], false, false, true, true),
        (vec![silent], false, false, false, true),
        (vec![Problem::DailyReportOverdue], false, true, false, true),
        (
            vec![reporting, "Brak papieru".into()],
            false,
            false,
            false,
            false,
        ),
    ] {
        let device = Device {
            vendor: Vendor::Posnet,
            model: String::new(),
            firmware: String::new(),
            unique_number: "TST0000000001".to_owned(),
            problems,
            vat: vec![],
            capabilities: Capabilities::default(),
        };
        let label = format!("{:?}", device.problems);
        assert_eq!(device.ready(), ready, "{label}");
        assert_eq!(device.daily_report_allowed(), daily, "{label}");
        assert_eq!(device.periodic_reports_allowed(), periodic, "{label}");
        assert_eq!(
            device.require_periodic_reports().is_ok(),
            periodic,
            "{label}"
        );
        assert_eq!(device.require_resolvable().is_ok(), resolvable, "{label}");
    }
    let read_only = Device {
        vendor: Vendor::Posnet,
        model: String::new(),
        firmware: String::new(),
        unique_number: "TST0000000001".to_owned(),
        problems: vec![Problem::ReadOnly {
            periodic_reports: false,
        }],
        vat: vec![],
        capabilities: Capabilities::default(),
    };
    assert_eq!(
        read_only.description(),
        "Drukarka jest w trybie tylko do odczytu"
    );
}

#[test]
fn fiscal_memory_unique_number_uses_exact_ascii_format() {
    for identity in ["ABC1234567890", "ZZZ0000000000"] {
        assert!(valid_unique_number(identity));
    }
    for identity in [
        "",
        "ABC123456789",
        "ABC12345678901",
        "abc1234567890",
        "AB11234567890",
        "ABC123456789x",
        "ĄBC1234567890",
        " ABC1234567890",
    ] {
        assert!(!valid_unique_number(identity));
    }
}
