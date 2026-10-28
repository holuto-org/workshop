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
    assert_eq!(
        sim.receipts(),
        [SimReceipt {
            total: 100,
            lines: vec![params(&[
                ("na", "Olej"),
                ("vt", "0"),
                ("pr", "100"),
                ("il", "1"),
                ("wa", "100")
            ])],
            payments: vec![params(&[("ty", "0"), ("wa", "100")])],
            buyer_nip: None,
        }]
    );
    assert_eq!(
        fiscal_commands(&sim),
        ["trinit", "trline", "trpayment", "trend"]
    );
    assert_eq!(journal.stages, ["trinit", "trline", "trpayment", "trend"]);
}

#[test]
fn demo_mode_keeps_all_other_preflight_gates() {
    for (field, value, problem) in [
        ("td", "1", "Drukarka jest w trybie tylko do odczytu"),
        ("hr", "0", "Drukarka nie ma zaprogramowanego nagłówka"),
        ("qe", "0", "Drukarka ma oczekujące polecenia"),
        (
            "to",
            "1",
            "Otwarta transakcja. Dokończ lub anuluj ją na drukarce",
        ),
        ("pr", "5", "Brak papieru"),
        (
            "nm",
            "OTHER",
            "Wybrane urządzenie nie jest drukarką Temo Online",
        ),
        ("nu", "", "Nieprawidłowy numer unikatowy drukarki"),
        (
            "tl",
            "1",
            "Zasoby transakcji są zajęte przez inny interfejs",
        ),
        (
            "fe",
            "1",
            "Otwarta transakcja. Dokończ lub anuluj ją na drukarce",
        ),
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
            Err(Error::NotReady(reason)) if reason.to_string() == problem
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
    assert_eq!(fiscal_commands(&sim), ["monthlyrep"]);
    assert!(!commands(&sim).iter().any(|command| command == "fmrecfisc"));
    assert_eq!(
        sim.reports(),
        [SimReport {
            command: "monthlyrep".to_owned(),
            params: params(&[("da", "2026-06-01"), ("su", "0")]),
        }]
    );
}
