use super::*;

#[test]
fn demo_mode_relaxes_only_fiscal_mode() {
    let sim = simulator();
    sim.set_status("fs", "0");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(expected.ready(), "{}", expected.description());
    let mut journal = Recorder::default();
    let printed = driver
        .print_receipt(&sale(), &expected, &mut journal)
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(sim.receipts().len(), 1);
    assert_eq!(
        fiscal_commands(&sim),
        ["trinit", "trline", "trpayment", "trend"]
    );
    assert_eq!(journal.stages, ["trinit", "trline", "trpayment", "trend"]);
}

#[test]
fn demo_mode_keeps_all_other_preflight_gates() {
    for (field, value) in [
        ("td", "1"),
        ("hr", "0"),
        ("qe", "0"),
        ("to", "1"),
        ("pr", "5"),
    ] {
        let sim = simulator();
        sim.set_status("fs", "0");
        sim.set_status(field, value);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(!expected.ready(), "{field}");
        let mut journal = Recorder::default();
        assert!(matches!(
            driver.print_receipt(&sale(), &expected, &mut journal),
            Err(Error::NotReady(_))
        ));
        assert_eq!(journal.stages, Vec::<&str>::new());
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
    }
}

#[test]
fn demo_mode_prints_reports_without_a_fiscalisation_record() {
    let sim = simulator();
    sim.set_status("fs", "0");
    let mut journal = Recorder::default();
    printer(&sim)
        .print_report(
            Report::Monthly {
                year: 2026,
                month: 6,
            },
            UNIQUE_NUMBER,
            &mut journal,
        )
        .unwrap();
    assert_eq!(journal.stages, ["monthlyrep"]);
    assert_eq!(sim.reports().len(), 1);
}
