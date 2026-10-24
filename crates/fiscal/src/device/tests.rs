use super::{VatKind, VatRate, Vendor};

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
    for (kind, label, active, exempt) in [
        (VatKind::Rate { basis_points: 2300 }, "A · 23%", true, false),
        (VatKind::Rate { basis_points: 850 }, "A · 8,5%", true, false),
        (VatKind::Rate { basis_points: 0 }, "A · 0%", true, false),
        (VatKind::Exempt, "A · zwolniona", true, true),
        (VatKind::Inactive, "A · nieaktywna", false, false),
    ] {
        let rate = VatRate { slot: 0, kind };
        assert_eq!(rate.label(), label);
        assert_eq!(rate.active(), active, "{label}");
        assert_eq!(rate.exempt(), exempt, "{label}");
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
