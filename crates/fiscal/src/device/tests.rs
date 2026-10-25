use super::{Capabilities, Device, Problem, VatKind, VatRate, Vendor};

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
                serial: String::new(),
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
