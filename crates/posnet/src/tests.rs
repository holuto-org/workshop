use std::io;

use jiff::civil::date;

use fiscal::{
    device::Problem,
    driver::Driver,
    error::Error,
    journal::Journal,
    link::Connection,
    money::Grosze,
    receipt::{Advance, PaymentForm, Receipt, VatSlot},
    report::Report,
};

use crate::{
    capabilities::capabilities,
    frame,
    printer::{Printer, identify},
    simulator::{Fault, SimReceipt, SimReport, Simulator, UNIQUE_NUMBER},
    testing::{BRAND, line, params, pay},
};

#[cfg(workshop_acceptance)]
mod acceptance;
mod output;
mod telemetry;

#[derive(Default)]
struct Recorder {
    stages: Vec<&'static str>,
    fail_at: Option<usize>,
}

impl Journal for Recorder {
    fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
        if self.fail_at == Some(self.stages.len()) {
            return Err(io::Error::other("disk full"));
        }
        self.stages.push(command);
        Ok(())
    }

    fn waiting(&mut self, _problem: Option<&str>) {}
}

fn simulator() -> Simulator {
    let sim = Simulator::start();
    sim.set_clock("2026-07-01T10:34:56Z".parse().unwrap());
    sim
}

fn printer(sim: &Simulator) -> Printer {
    Printer::new(
        sim.link(),
        Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    )
}

fn sale() -> Receipt {
    Receipt::new(
        vec![line("Olej", "1", "1", 0)],
        vec![],
        vec![pay(PaymentForm::Cash, 100)],
        None,
        BRAND,
        &capabilities(),
    )
    .unwrap()
}

fn all_steps_receipt() -> Receipt {
    Receipt::new(
        vec![line("Olej", "1", "2", 0)],
        vec![Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(100).unwrap(),
        }],
        vec![pay(PaymentForm::Card, 100)],
        Some("1234567890".to_owned()),
        "Inna marka",
        &capabilities(),
    )
    .unwrap()
}

fn all_steps_effect() -> SimReceipt {
    SimReceipt {
        total: 100,
        lines: vec![params(&[
            ("na", "Olej"),
            ("vt", "0"),
            ("pr", "200"),
            ("il", "1"),
            ("wa", "200"),
        ])],
        payments: vec![params(&[("ty", "2"), ("wa", "100")])],
        buyer_nip: Some("1234567890".to_owned()),
    }
}

fn report_effect(command: &str) -> SimReport {
    SimReport {
        command: command.to_owned(),
        params: params(match command {
            "dailyrep" => &[("da", "2026-07-01")],
            "monthlyrep" => &[("da", "2026-06-01"), ("su", "0")],
            "periodicrepbydates" => &[("fd", "2026-01-01"), ("td", "2026-06-30"), ("su", "0")],
            _ => panic!("unknown report fixture"),
        }),
    }
}

fn commands(sim: &Simulator) -> Vec<String> {
    sim.frames()
        .iter()
        .map(|raw| frame::decode(raw).unwrap().0)
        .collect()
}

fn fiscal_commands(sim: &Simulator) -> Vec<String> {
    commands(sim)
        .into_iter()
        .filter(|command| {
            command.starts_with("tr") || command.ends_with("rep") || command == "periodicrepbydates"
        })
        .collect()
}

#[cfg(not(workshop_acceptance))]
#[test]
fn non_fiscal_device_is_not_ready_without_acceptance() {
    let sim = simulator();
    sim.set_status("fs", "0");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.ready());
    let mut journal = Recorder::default();
    assert!(matches!(
        driver.print_receipt(&sale(), &expected, &mut journal),
        Err(Error::NotReady(_))
    ));
    assert_eq!(journal.stages, Vec::<&str>::new());
    assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
}

#[test]
fn every_preflight_problem_refuses_receipts_and_reports_before_journaling() {
    for (field, value, problem) in [
        (
            "nm",
            "OTHER",
            "Wybrane urządzenie nie jest drukarką Temo Online",
        ),
        ("nu", "", "Nieprawidłowy numer unikatowy drukarki"),
        #[cfg(not(workshop_acceptance))]
        ("fs", "0", "Drukarka nie jest w trybie fiskalnym"),
        ("hr", "0", "Drukarka nie ma zaprogramowanego nagłówka"),
        ("ds", "1", "Drukarka oczekuje na operatora lub jest w menu"),
        ("ds", "2", "Drukarka oczekuje na operatora lub jest w menu"),
        ("ds", "3", "Drukarka oczekuje na operatora lub jest w menu"),
        ("qe", "0", "Drukarka ma oczekujące polecenia"),
        ("pr", "1", "Podniesiona dźwignia"),
        ("pr", "2", "Błąd mechanizmu: 2"),
        ("pr", "3", "Otwarta pokrywa"),
        ("pr", "4", "Błąd mechanizmu: 4"),
        ("pr", "5", "Brak papieru"),
        ("pr", "6", "Nieprawidłowa temperatura lub zasilanie"),
        ("pr", "7", "Błąd mechanizmu: 7"),
        ("pr", "8", "Błąd mechanizmu: 8"),
        ("pr", "9", "Błąd mechanizmu: 9"),
        ("pr", "10", "Błąd mechanizmu: 10"),
        (
            "to",
            "1",
            "Otwarta transakcja. Dokończ lub anuluj ją na drukarce",
        ),
        (
            "fe",
            "1",
            "Otwarta transakcja. Dokończ lub anuluj ją na drukarce",
        ),
        (
            "ts",
            "16",
            "Otwarta transakcja. Dokończ lub anuluj ją na drukarce",
        ),
        ("td", "1", "Drukarka jest w trybie tylko do odczytu"),
        (
            "tl",
            "1",
            "Zasoby transakcji są zajęte przez inny interfejs",
        ),
    ] {
        let sim = simulator();
        let device = printer(&sim);
        let expected = device.probe().unwrap();
        sim.set_status(field, value);
        let mut journal = Recorder::default();
        let error = device
            .print_receipt(&sale(), &expected, &mut journal)
            .unwrap_err();
        assert!(
            matches!(&error,Error::NotReady(reason) if reason == problem),
            "{field}={value}: {error:?}"
        );
        let report = device
            .print_report(Report::Daily, UNIQUE_NUMBER, &mut journal)
            .unwrap_err();
        assert!(
            matches!(&report,Error::NotReady(reason) if reason == problem),
            "{field}={value}: {report:?}"
        );
        assert_eq!(journal.stages, Vec::<&str>::new(), "{field}={value}");
        assert_eq!(
            fiscal_commands(&sim),
            Vec::<String>::new(),
            "{field}={value}"
        );
    }
}

#[test]
fn an_enabled_posnet_interface_without_windows_1250_refuses_before_journaling() {
    for (interface, refused) in [
        ([("id", "2"), ("pr", "0"), ("cp", "1")], true),
        ([("id", "1"), ("pr", "0"), ("cp", "2")], true),
        // Disabled and Thermal-protocol interfaces cannot carry this session.
        ([("id", "0"), ("pr", "0"), ("cp", "2")], false),
        ([("id", "2"), ("pr", "1"), ("cp", "2")], false),
    ] {
        let sim = simulator();
        let device = printer(&sim);
        let expected = device.probe().unwrap();
        sim.fault(
            "pccfgget",
            Fault::Reply(
                interface
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            ),
        );
        let mut journal = Recorder::default();
        let receipt = device.print_receipt(&sale(), &expected, &mut journal);
        let report = device.print_report(Report::Daily, UNIQUE_NUMBER, &mut journal);
        if refused {
            for result in [receipt.map(|_| ()), report] {
                assert!(
                    matches!(&result, Err(Error::NotReady(reason))
                        if reason == "Drukarka nie używa strony kodowej Windows-1250"),
                    "{interface:?}: {result:?}"
                );
            }
            assert_eq!(journal.stages, Vec::<&str>::new(), "{interface:?}");
            assert_eq!(fiscal_commands(&sim), Vec::<String>::new(), "{interface:?}");
        } else {
            let _printed = receipt.unwrap();
            report.unwrap();
        }
    }
    // Each interface's own reply decides: neither can hide the other.
    for (pc1, pc2) in [("0", "2"), ("1", "0")] {
        let sim = simulator();
        sim.set_code_page("1", pc1);
        sim.set_code_page("2", pc2);
        let device = printer(&sim);
        let problems = device.probe().unwrap().problems;
        assert!(
            matches!(problems.as_slice(), [Problem::Blocking(reason)]
                if reason == "Drukarka nie używa strony kodowej Windows-1250"),
            "PC1 cp{pc1}, PC2 cp{pc2}: {problems:?}"
        );
    }
}

#[test]
fn every_incomplete_preflight_reply_refuses_before_journaling_or_fiscal_effects() {
    for (command, field) in [
        ("getrealid", "nm"),
        ("sdev", "qe"),
        ("sprn", "pr"),
        ("scomm", "fs"),
        ("strns", "to"),
        ("vatget", "va"),
        ("pccfgget", "id"),
    ] {
        let sim = simulator();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault(command, Fault::Reply(vec![]));
        let mut journal = Recorder::default();
        for error in [
            driver
                .print_receipt(&sale(), &expected, &mut journal)
                .unwrap_err(),
            driver
                .print_report(Report::Daily, UNIQUE_NUMBER, &mut journal)
                .unwrap_err(),
            driver.verify(UNIQUE_NUMBER).unwrap_err(),
        ] {
            assert!(
                matches!(&error, Error::Protocol(message) if message ==
                    &format!("Niepełna lub nieprawidłowa odpowiedź statusu drukarki: \
                              Niepełna odpowiedź drukarki: brak parametru {field}.")),
                "{command}: {error:?}"
            );
        }
        assert_eq!(journal.stages, Vec::<&str>::new());
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
        assert_eq!(sim.receipts(), vec![]);
        assert_eq!(sim.reports(), vec![]);
    }
}

#[test]
fn undocumented_false_status_spellings_never_authorize_a_fiscal_command() {
    for field in ["fs", "hr", "qe", "fe", "td"] {
        for value in ["f", "F"] {
            let sim = simulator();
            let device = printer(&sim);
            let expected = device.probe().unwrap();
            sim.set_status(field, value);
            let mut journal = Recorder::default();
            assert!(
                matches!(
                    device.print_receipt(&sale(), &expected, &mut journal),
                    Err(Error::Protocol(_))
                ),
                "{field}={value}"
            );
            assert!(
                matches!(
                    device.print_report(Report::Daily, UNIQUE_NUMBER, &mut journal),
                    Err(Error::Protocol(_))
                ),
                "{field}={value}"
            );
            assert_eq!(journal.stages, Vec::<&str>::new());
            assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
            assert_eq!(sim.receipts(), vec![]);
            assert_eq!(sim.reports(), vec![]);
        }
    }
}

#[test]
fn malformed_zero_daily_error_codes_remain_uncertain_and_are_not_retried() {
    let sim = simulator();
    sim.fault(
        "dailyrep",
        Fault::Reply(vec![("?".to_owned(), "+382".to_owned())]),
    );
    let mut journal = Recorder::default();
    let error = printer(&sim)
        .print_report(Report::Daily, UNIQUE_NUMBER, &mut journal)
        .unwrap_err();
    assert!(
        matches!(&error, Error::Uncertain(inner) if matches!(inner.as_ref(), Error::Protocol(_))),
        "{error:?}"
    );
    assert_eq!(journal.stages, ["dailyrep"]);
    assert_eq!(fiscal_commands(&sim), ["dailyrep"]);
    assert_eq!(sim.reports(), vec![]);
}

#[test]
fn inactive_missing_or_changed_vat_is_refused_before_trinit() {
    for mode in ["inactive", "changed", "regraded", "none", "missing"] {
        let sim = simulator();
        let device = printer(&sim);
        let expected = device.probe().unwrap();
        let receipt = if mode == "inactive" {
            Receipt::new(
                vec![line("Olej", "1", "1", 5)],
                vec![],
                vec![pay(PaymentForm::Cash, 100)],
                None,
                BRAND,
                &capabilities(),
            )
            .unwrap()
        } else {
            sale()
        };
        match mode {
            "changed" => sim.set_vat([
                "22,00", "8,00", "0,00", "101,00", "101,00", "101,00", "100,00",
            ]),
            // Slot G may hold any rate, so the device stays ready; only the VAT guard refuses.
            "regraded" => {
                sim.set_vat(["23,00", "8,00", "5,00", "0,00", "100,00", "101,00", "8,00"]);
            },
            "none" => sim.set_vat(["101,00"; 7]),
            "missing" => sim.fault("vatget", Fault::Reply(vec![])),
            _ => {},
        }
        let error = device
            .print_receipt(&receipt, &expected, &mut Recorder::default())
            .unwrap_err();
        match mode {
            "inactive" => assert_eq!(
                error.to_string(),
                "Wybrana stawka VAT jest nieaktywna na drukarce."
            ),
            "changed" => assert_eq!(error.to_string(), "Nieprawidłowe przypisanie liter VAT A–E"),
            "regraded" => assert_eq!(
                error.to_string(),
                "Stawka VAT zmieniła się na drukarce. Odśwież połączenie i sprawdź stawki."
            ),
            "none" => assert_eq!(error.to_string(), "Brak aktywnych stawek VAT"),
            "missing" => assert!(matches!(error, Error::Protocol(_)), "{error:?}"),
            _ => panic!("unknown fixture mode"),
        }
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
    }
}

#[test]
fn a_device_swap_refuses_receipts_reports_and_resolution() {
    let sim = simulator();
    let device = printer(&sim);
    let mut expected = device.probe().unwrap();
    expected.unique_number = "OLD".to_owned();
    for error in [
        device
            .print_receipt(&sale(), &expected, &mut Recorder::default())
            .unwrap_err(),
        device
            .print_report(Report::Daily, "OLD", &mut Recorder::default())
            .unwrap_err(),
        device.verify("OLD").unwrap_err(),
    ] {
        assert_eq!(
            error.to_string(),
            "Podłączona drukarka DEM0000000001 nie jest drukarką OLD, której dotyczy operacja."
        );
    }
    assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
}

#[test]
fn unreadable_post_print_counters_do_not_turn_a_completed_receipt_into_a_failure() {
    let sim = simulator();
    sim.fault("scnt", Fault::Error);
    let device = printer(&sim);
    let printed = device
        .print_receipt(&sale(), &device.probe().unwrap(), &mut Recorder::default())
        .unwrap();
    assert_eq!(printed.receipt_number, None);
    assert_eq!(printed.issued_at, "2026-07-01T10:34:00Z".parse().unwrap());
    assert_eq!(sim.receipts()[0].total, 100);
    assert_eq!(
        fiscal_commands(&sim),
        ["trinit", "trline", "trpayment", "trend"]
    );
}

#[test]
fn the_device_clock_preserves_both_dst_offsets_and_truncates_seconds() {
    for (clock, expected) in [
        ("2026-03-29T00:59:59Z", "2026-03-29T00:59:00Z"),
        ("2026-03-29T01:00:01Z", "2026-03-29T01:00:00Z"),
        ("2026-10-25T00:30:59Z", "2026-10-25T00:30:00Z"),
        ("2026-10-25T01:30:59Z", "2026-10-25T01:30:00Z"),
    ] {
        let sim = simulator();
        sim.set_clock(clock.parse().unwrap());
        let device = printer(&sim);
        let printed = device
            .print_receipt(&sale(), &device.probe().unwrap(), &mut Recorder::default())
            .unwrap();
        assert_eq!(printed.issued_at, expected.parse().unwrap(), "{clock}");
    }
}

#[test]
fn legacy_clock_without_an_offset_chooses_the_earlier_repeated_instant() {
    let sim = simulator();
    sim.fault(
        "rtcget",
        Fault::Reply(vec![("da".to_owned(), "2026-10-25;02:30".to_owned())]),
    );
    let device = printer(&sim);
    assert_eq!(
        device
            .print_receipt(&sale(), &device.probe().unwrap(), &mut Recorder::default())
            .unwrap()
            .issued_at,
        "2026-10-25T00:30:00Z".parse().unwrap()
    );
}

#[test]
fn a_nonexistent_legacy_clock_time_refuses_receipts_but_keeps_date_only_reports() {
    let sim = simulator();
    sim.fault(
        "rtcget",
        Fault::Reply(vec![("da".to_owned(), "2026-03-29;02:30".to_owned())]),
    );
    let device = printer(&sim);
    let expected = device.probe().unwrap();
    let mut journal = Recorder::default();
    let error = device
        .print_receipt(&sale(), &expected, &mut journal)
        .unwrap_err();
    assert!(
        matches!(&error, Error::Protocol(message)
            if message == "Nie można odczytać zegara drukarki: \
                           czas lokalny nie istnieje w strefie Europe/Warsaw."),
        "{error:?}"
    );
    assert_eq!(journal.stages, Vec::<&str>::new());
    assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
    assert_eq!(sim.receipts(), vec![]);
    device
        .print_report(Report::Daily, UNIQUE_NUMBER, &mut journal)
        .unwrap();
    assert_eq!(journal.stages, ["dailyrep"]);
    assert_eq!(
        sim.reports(),
        [SimReport {
            command: "dailyrep".to_owned(),
            params: params(&[("da", "2026-03-29")]),
        }]
    );
}

#[test]
fn unreadable_clocks_fail_before_fiscal_commands() {
    for fault in [
        Fault::Error,
        Fault::Reply(vec![]),
        Fault::Reply(vec![("da".to_owned(), "nonsense".to_owned())]),
        Fault::Reply(vec![
            ("tm".to_owned(), "nonsense".to_owned()),
            ("da".to_owned(), "2026-07-01;12:34".to_owned()),
        ]),
    ] {
        let sim = simulator();
        sim.fault("rtcget", fault);
        let device = printer(&sim);
        let error = device
            .print_receipt(&sale(), &device.probe().unwrap(), &mut Recorder::default())
            .unwrap_err();
        assert!(!matches!(error, Error::Uncertain(_)), "{error:?}");
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
    }
}

#[test]
fn header_formatting_and_case_choose_only_the_needed_full_company_footer() {
    for (header, footer) in [
        ("&c&1AUTO && MOTO&1&c\nŻółw&", false),
        ("&cAuto &b&&&b Moto&c", false),
        ("Auto && Moto dodatkowy tekst", false),
        ("Auto", true),
        ("Auto && Motocykl", false),
        ("Auto && Mot", true),
    ] {
        let sim = simulator();
        sim.set_header(header);
        let device = printer(&sim);
        let expected = device.probe().unwrap();
        let receipt = Receipt::new(
            vec![line("Olej", "1", "1", 0)],
            vec![],
            vec![pay(PaymentForm::Cash, 100)],
            None,
            "Auto & Moto",
            &capabilities(),
        )
        .unwrap();
        let mut journal = Recorder::default();
        let printed = device
            .print_receipt(&receipt, &expected, &mut journal)
            .unwrap();
        assert_eq!(printed.receipt_number.as_deref(), Some("1"));
        let expected_commands = if footer {
            vec![
                "trinit",
                "trline",
                "trpayment",
                "trend",
                "trftrln",
                "trftrend",
            ]
        } else {
            vec!["trinit", "trline", "trpayment", "trend"]
        };
        assert_eq!(fiscal_commands(&sim), expected_commands, "{header:?}");
        assert_eq!(journal.stages, expected_commands, "{header:?}");
        if footer {
            let fields = sim
                .frames()
                .iter()
                .map(|raw| frame::decode(raw).unwrap())
                .find(|(command, _)| command == "trftrln")
                .unwrap()
                .1;
            assert_eq!(
                fields.fields(),
                &[
                    ("id".to_owned(), "25".to_owned()),
                    ("na".to_owned(), "Auto & Moto".to_owned()),
                ]
            );
        }
        assert!(device.probe().unwrap().ready(), "{header:?}");
    }
}

#[test]
fn a_report_journal_failure_sends_no_fiscal_command_or_effect() {
    for report in [
        Report::Daily,
        Report::Monthly {
            year: 2026,
            month: 6,
        },
        Report::Periodic {
            from: date(2026, 1, 1),
            to: date(2026, 6, 30),
            summary: true,
        },
    ] {
        let sim = simulator();
        let mut journal = Recorder {
            fail_at: Some(0),
            ..Recorder::default()
        };
        assert!(
            matches!(
                printer(&sim).print_report(report, UNIQUE_NUMBER, &mut journal),
                Err(Error::Journal(_))
            ),
            "{report:?}"
        );
        assert_eq!(journal.stages, Vec::<&str>::new());
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
        assert_eq!(sim.reports(), vec![]);
    }
}

#[test]
fn refused_trinit_and_report_commands_are_certain_but_other_named_errors_are_not() {
    for (command, report) in [
        ("trinit", None),
        ("dailyrep", Some(Report::Daily)),
        (
            "monthlyrep",
            Some(Report::Monthly {
                year: 2026,
                month: 6,
            }),
        ),
        (
            "periodicrepbydates",
            Some(Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 6, 30),
                summary: false,
            }),
        ),
    ] {
        for named in [command, "!sdev"] {
            let sim = simulator();
            sim.fault(
                command,
                Fault::Device {
                    code: 2036,
                    command: named.to_owned(),
                    field: "da".to_owned(),
                },
            );
            let device = printer(&sim);
            let status = device.probe().unwrap();
            let mut journal = Recorder::default();
            let error = if let Some(report) = report {
                device
                    .print_report(report, UNIQUE_NUMBER, &mut journal)
                    .unwrap_err()
            } else {
                device
                    .print_receipt(&sale(), &status, &mut journal)
                    .unwrap_err()
            };
            if named == command {
                assert!(
                    matches!(error, Error::Device { code: 2036, .. }),
                    "{command}: {error:?}"
                );
            } else {
                assert!(matches!(error, Error::Uncertain(_)), "{command}: {error:?}");
            }
            assert_eq!(journal.stages, [command]);
            assert_eq!(fiscal_commands(&sim), [command]);
            assert_eq!(sim.receipts(), vec![]);
            assert_eq!(sim.reports(), vec![]);
        }
    }
}

#[test]
fn a_journal_failure_prevents_the_next_send_and_keeps_prior_effects_uncertain() {
    for (index, expected) in [
        (0, vec![]),
        (1, vec!["trinit"]),
        (2, vec!["trinit", "trline"]),
    ] {
        let sim = simulator();
        let device = printer(&sim);
        let status = device.probe().unwrap();
        let mut journal = Recorder {
            fail_at: Some(index),
            ..Recorder::default()
        };
        let error = device
            .print_receipt(&sale(), &status, &mut journal)
            .unwrap_err();
        assert!(
            if index == 0 {
                matches!(error, Error::Journal(_))
            } else {
                matches!(error, Error::Uncertain(_))
            },
            "index {index}: {error:?}"
        );
        assert_eq!(fiscal_commands(&sim), expected);
        assert_eq!(journal.stages, expected);
    }
}

#[test]
fn invalid_reports_are_refused_without_sending_a_report() {
    for report in [
        Report::Monthly {
            year: 2026,
            month: 13,
        },
        Report::Periodic {
            from: date(2026, 7, 2),
            to: date(2026, 7, 1),
            summary: false,
        },
    ] {
        let sim = simulator();
        assert!(
            matches!(
                printer(&sim).print_report(report, UNIQUE_NUMBER, &mut Recorder::default()),
                Err(Error::Invalid(_))
            ),
            "{report:?}"
        );
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
    }
}

#[test]
fn reports_outside_the_printer_date_and_fiscalisation_are_refused_before_journaling() {
    for (report, message) in [
        (
            Report::Monthly {
                year: 2026,
                month: 8,
            },
            "Raport nie może zaczynać się po dacie drukarki (01.07.2026).",
        ),
        (
            Report::Periodic {
                from: date(2026, 7, 2),
                to: date(2026, 7, 2),
                summary: false,
            },
            "Raport nie może zaczynać się po dacie drukarki (01.07.2026).",
        ),
        (
            Report::Monthly {
                year: 2026,
                month: 2,
            },
            "Raport nie może kończyć się przed fiskalizacją drukarki (10.03.2026).",
        ),
        (
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 3, 9),
                summary: true,
            },
            "Raport nie może kończyć się przed fiskalizacją drukarki (10.03.2026).",
        ),
    ] {
        let sim = simulator();
        sim.set_fiscalised("2026-03-10T08:00:00Z".parse().unwrap());
        let mut journal = Recorder::default();
        let error = printer(&sim)
            .print_report(report, UNIQUE_NUMBER, &mut journal)
            .unwrap_err();
        assert!(
            matches!(&error, Error::Invalid(reason) if reason == message),
            "{report:?}: {error:?}"
        );
        assert_eq!(journal.stages, Vec::<&str>::new(), "{report:?}");
        assert_eq!(fiscal_commands(&sim), Vec::<String>::new(), "{report:?}");
        assert_eq!(sim.reports(), vec![]);
    }
}

#[test]
fn reports_include_the_current_month_and_fiscalisation_day_boundaries() {
    for (report, command, fields) in [
        (
            Report::Monthly {
                year: 2026,
                month: 7,
            },
            "monthlyrep",
            params(&[("da", "2026-07-01"), ("su", "0")]),
        ),
        (
            Report::Monthly {
                year: 2026,
                month: 3,
            },
            "monthlyrep",
            params(&[("da", "2026-03-01"), ("su", "0")]),
        ),
        (
            Report::Periodic {
                from: date(2026, 7, 1),
                to: date(2026, 7, 1),
                summary: false,
            },
            "periodicrepbydates",
            params(&[("fd", "2026-07-01"), ("td", "2026-07-01"), ("su", "0")]),
        ),
        (
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 3, 10),
                summary: true,
            },
            "periodicrepbydates",
            params(&[("fd", "2026-01-01"), ("td", "2026-03-10"), ("su", "1")]),
        ),
    ] {
        let sim = simulator();
        sim.set_fiscalised("2026-03-10T08:00:00Z".parse().unwrap());
        let mut journal = Recorder::default();
        printer(&sim)
            .print_report(report, UNIQUE_NUMBER, &mut journal)
            .unwrap();
        assert_eq!(journal.stages, [command], "{report:?}");
        assert_eq!(fiscal_commands(&sim), [command], "{report:?}");
        assert_eq!(
            sim.reports(),
            [SimReport {
                command: command.to_owned(),
                params: fields
            }],
            "{report:?}"
        );
    }
}

#[test]
fn unidentified_devices_receive_only_the_identification_command() {
    let sim = simulator();
    sim.set_status("nm", "OTHER");
    assert!(
        identify(
            sim.link().as_ref(),
            &Connection {
                port: sim.address().to_owned(),
                baud_rate: 9600,
            },
        )
        .unwrap()
        .is_none(),
        "foreign device is not adopted"
    );
    assert_eq!(commands(&sim), ["getrealid"]);
}

#[test]
fn an_unsupported_posnet_model_is_reported_by_name_after_only_identification() {
    let sim = simulator();
    sim.set_status("nm", "POSNET THERMAL HD ONLINE");
    let result = identify(
        sim.link().as_ref(),
        &Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    );
    assert!(
        matches!(
            &result,
            Err(Error::Unsupported(message)) if message
                == "Nieobsługiwany model drukarki Posnet: POSNET THERMAL HD ONLINE \
                    (oprogramowanie 32.01). Obsługiwana jest Posnet Temo Online."
        ),
        "{result:?}"
    );
    assert_eq!(commands(&sim), ["getrealid"]);
}

/// Recognizes the transport error a reply fault surfaces as.
type Surfaced = fn(&Error) -> bool;

/// Faults whose reply failure leaves the executed command's effect uncertain.
fn reply_faults() -> [(Fault, Surfaced); 3] {
    [
        (Fault::LoseReply, |error| matches!(error, Error::Timeout(_))),
        (Fault::CorruptReply, |error| {
            matches!(error, Error::Protocol(_))
        }),
        (Fault::Disconnect, |error| matches!(error, Error::Io(_))),
    ]
}

#[test]
fn a_lost_corrupt_or_disconnected_receipt_reply_is_uncertain_and_never_resent() {
    let expected = [
        "trinit",
        "trnipset",
        "trline",
        "tradvance",
        "trpayment",
        "trend",
        "trftrln",
        "trftrend",
    ];
    for (end, command) in expected.into_iter().enumerate().take(6) {
        for (fault, surfaced) in reply_faults() {
            let case = format!("{command} {fault:?}");
            let sim = simulator();
            sim.fault(command, fault);
            let device = printer(&sim);
            let status = device.probe().unwrap();
            let mut journal = Recorder::default();
            let error = device
                .print_receipt(&all_steps_receipt(), &status, &mut journal)
                .unwrap_err();
            assert!(
                matches!(&error, Error::Uncertain(inner) if surfaced(inner)),
                "{case}: {error:?}"
            );
            assert_eq!(
                fiscal_commands(&sim),
                expected[..=end],
                "{case}: send each command only once and stop at the failure"
            );
            assert_eq!(
                journal.stages,
                expected[..=end],
                "{case}: journal every send before execution"
            );
            assert_eq!(
                sim.transaction_open(),
                end < 5,
                "{case}: the command executed before its reply failed"
            );
            assert_eq!(
                sim.receipts(),
                if end >= 5 {
                    vec![all_steps_effect()]
                } else {
                    vec![]
                },
                "{case}"
            );
        }
    }
}

#[test]
fn a_lost_corrupt_or_disconnected_report_reply_is_uncertain_and_never_resent() {
    for (command, report) in [
        ("dailyrep", Report::Daily),
        (
            "monthlyrep",
            Report::Monthly {
                year: 2026,
                month: 6,
            },
        ),
        (
            "periodicrepbydates",
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 6, 30),
                summary: false,
            },
        ),
    ] {
        for (fault, _surfaced) in reply_faults() {
            let case = format!("{command} {fault:?}");
            let sim = simulator();
            sim.fault(command, fault);
            let mut journal = Recorder::default();
            let error = printer(&sim)
                .print_report(report, UNIQUE_NUMBER, &mut journal)
                .unwrap_err();
            assert!(matches!(error, Error::Uncertain(_)), "{case}: {error:?}");
            assert_eq!(
                fiscal_commands(&sim),
                [command],
                "{case}: never resend a report"
            );
            assert_eq!(journal.stages, [command], "{case}");
            assert_eq!(
                sim.reports(),
                [report_effect(command)],
                "{case}: report executed before the reply failed"
            );
        }
    }
}

#[test]
fn a_refused_later_receipt_step_is_uncertain_and_stops_the_stream() {
    for (index, command) in ["trnipset", "trline", "tradvance", "trpayment", "trend"]
        .into_iter()
        .enumerate()
    {
        let sim = simulator();
        sim.fault(command, Fault::Error);
        let device = printer(&sim);
        let status = device.probe().unwrap();
        let mut journal = Recorder::default();
        let error = device
            .print_receipt(&all_steps_receipt(), &status, &mut journal)
            .unwrap_err();
        let Error::Uncertain(inner) = &error else {
            panic!("{error:?}")
        };
        assert!(
            matches!(inner.as_ref(),
                Error::Device {code:2000,command:named,..} if named == command),
            "{error:?}"
        );
        let expected = [
            "trinit",
            "trnipset",
            "trline",
            "tradvance",
            "trpayment",
            "trend",
            "trftrln",
            "trftrend",
        ];
        assert_eq!(fiscal_commands(&sim), expected[..=index + 1]);
        assert_eq!(journal.stages, expected[..=index + 1]);
    }
}

#[test]
fn a_footer_failure_after_trend_returns_the_printed_receipt_without_a_second_send() {
    // `trend` completes the fiscal record (`strns.fe`, p. 377); the footer cannot undo it.
    let expected = [
        "trinit",
        "trnipset",
        "trline",
        "tradvance",
        "trpayment",
        "trend",
        "trftrln",
        "trftrend",
    ];
    for (index, command) in [(6, "trftrln"), (7, "trftrend")] {
        for fault in [
            None,
            Some(Fault::Error),
            Some(Fault::LoseReply),
            Some(Fault::CorruptReply),
            Some(Fault::Disconnect),
        ] {
            let sim = simulator();
            let mut journal = Recorder::default();
            match &fault {
                Some(fault) => sim.fault(command, fault.clone()),
                None => journal.fail_at = Some(index),
            }
            let device = printer(&sim);
            let status = device.probe().unwrap();
            let printed = device
                .print_receipt(&all_steps_receipt(), &status, &mut journal)
                .unwrap_or_else(|error| panic!("{command} {fault:?}: {error:?}"));
            assert_eq!(printed.issued_at, "2026-07-01T10:34:00Z".parse().unwrap());
            assert_eq!(
                printed.receipt_number.as_deref(),
                (fault != Some(Fault::Disconnect)).then_some("1"),
                "{command} {fault:?}: the counter is read whenever the link survives"
            );
            let sent = if fault.is_some() { index + 1 } else { index };
            assert_eq!(
                fiscal_commands(&sim),
                expected[..sent],
                "{command} {fault:?}: stop at the failure without resending"
            );
            assert_eq!(journal.stages, expected[..sent]);
            assert_eq!(sim.receipts(), [all_steps_effect()]);
            let footer_closed = command == "trftrend"
                && matches!(
                    fault,
                    Some(Fault::LoseReply | Fault::CorruptReply | Fault::Disconnect)
                );
            assert_eq!(
                device.probe().unwrap().ready(),
                footer_closed,
                "{command} {fault:?}: preflight reports an unfinished footer"
            );
        }
    }
}

#[test]
fn every_fiscal_command_is_journaled_before_it_reaches_the_device() {
    struct Fence<'a> {
        sim: &'a Simulator,
        stages: Vec<&'static str>,
    }
    impl Journal for Fence<'_> {
        fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
            if fiscal_commands(self.sim) != self.stages {
                return Err(io::Error::other("command arrived before its intent"));
            }
            self.stages.push(command);
            Ok(())
        }

        fn waiting(&mut self, _problem: Option<&str>) {}
    }
    let sim = simulator();
    let device = printer(&sim);
    let status = device.probe().unwrap();
    let mut journal = Fence {
        sim: &sim,
        stages: vec![],
    };
    let printed = device
        .print_receipt(&all_steps_receipt(), &status, &mut journal)
        .unwrap();
    assert_eq!(
        journal.stages,
        [
            "trinit",
            "trnipset",
            "trline",
            "tradvance",
            "trpayment",
            "trend",
            "trftrln",
            "trftrend"
        ]
    );
    assert_eq!(fiscal_commands(&sim), journal.stages);
    assert_eq!(sim.receipts(), [all_steps_effect()]);
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(printed.issued_at, "2026-07-01T10:34:00Z".parse().unwrap());
    assert!(device.probe().unwrap().ready());
}

#[test]
fn a_report_execution_error_keeps_the_effect_uncertain_without_a_second_send() {
    for (command, report) in [
        ("dailyrep", Report::Daily),
        (
            "monthlyrep",
            Report::Monthly {
                year: 2026,
                month: 6,
            },
        ),
        (
            "periodicrepbydates",
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 6, 30),
                summary: false,
            },
        ),
    ] {
        let sim = simulator();
        sim.fault(command, Fault::ExecutionError);
        let mut journal = Recorder::default();
        let error = printer(&sim)
            .print_report(report, UNIQUE_NUMBER, &mut journal)
            .unwrap_err();
        let Error::Uncertain(inner) = &error else {
            panic!("{error:?}")
        };
        assert!(
            matches!(inner.as_ref(),
                Error::Device {code:2000, rejected:false, command:named, ..} if named == command),
            "{error:?}"
        );
        assert_eq!(fiscal_commands(&sim), [command]);
        assert_eq!(journal.stages, [command]);
        assert_eq!(sim.reports(), [report_effect(command)]);
    }
}

#[test]
fn a_trinit_execution_error_has_no_receipt_and_its_open_transaction_blocks_preflight() {
    let sim = simulator();
    sim.fault("trinit", Fault::ExecutionError);
    let device = printer(&sim);
    let expected = device.probe().unwrap();
    let mut journal = Recorder::default();
    let error = device
        .print_receipt(&sale(), &expected, &mut journal)
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Device {
                code: 2000,
                rejected: false,
                ..
            }
        ),
        "no trend was sent, so there is provably no fiscal receipt: {error:?}"
    );
    assert_eq!(sim.receipts(), vec![]);
    assert!(
        sim.transaction_open(),
        "preflight must catch the remaining transaction"
    );
    assert_eq!(fiscal_commands(&sim), ["trinit"]);
    assert_eq!(journal.stages, ["trinit"]);
    assert_eq!(
        device.verify(UNIQUE_NUMBER).unwrap_err().to_string(),
        "Otwarta transakcja. Dokończ lub anuluj ją na drukarce"
    );
    assert_eq!(
        fiscal_commands(&sim),
        ["trinit"],
        "verification prints nothing"
    );
}

#[test]
fn sale_blocks_the_preflight_cannot_see_tell_the_operator_what_to_fix() {
    for (command, code, shown) in [
        (
            "trinit",
            383,
            "Drukarka zwróciła błąd 383 (trinit). Brak raportu dobowego za poprzedni dzień \
             sprzedaży. Wykonaj raport dobowy.",
        ),
        (
            "trline",
            2106,
            "Drukarka zwróciła błąd 2106 (trline). Towar zablokowany: tę nazwę sprzedano \
             wcześniej w niższej stawce VAT, a drukarka nie pozwala jej podwyższyć. Zmień nazwę \
             pozycji.\nWynik operacji wymaga sprawdzenia. Nie wysyłaj jej ponownie przed \
             sprawdzeniem drukarki.",
        ),
    ] {
        let sim = simulator();
        sim.fault(command, Fault::Refused(code));
        let device = printer(&sim);
        let expected = device.probe().unwrap();
        let error = device
            .print_receipt(&sale(), &expected, &mut Recorder::default())
            .unwrap_err();
        assert_eq!(error.to_string(), shown, "{command} {code}");
        assert_eq!(sim.receipts(), vec![]);
    }
}

#[test]
fn a_spaced_unique_number_binds_in_its_canonical_form() {
    for (reported, bound, ready) in [
        ("DEM 0000000001", UNIQUE_NUMBER, true),
        ("DE M0000000001", "DE M0000000001", false),
        ("DEM 000000001", "DEM 000000001", false),
    ] {
        let sim = simulator();
        sim.set_status("nu", reported);
        let device = printer(&sim);
        let status = device.probe().unwrap();
        assert_eq!(status.unique_number, bound, "{reported:?}");
        assert_eq!(status.ready(), ready, "{reported:?}");
        if ready {
            let _printed = device
                .print_receipt(&sale(), &status, &mut Recorder::default())
                .unwrap();
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
        }
    }
}

#[test]
fn item_names_the_printer_treats_as_empty_are_refused_before_trinit() {
    for (name, refused) in [
        ("-", true),
        ("***", true),
        ("<:;__?[^^]__ ?;:>", true),
        ("%", false),
        ("ż", false),
        ("Ä", false),
        ("Č", false),
        ("- 5 -", false),
    ] {
        let sim = simulator();
        let device = printer(&sim);
        let status = device.probe().unwrap();
        let receipt = Receipt::new(
            vec![line(name, "1", "1", 0)],
            vec![],
            vec![pay(PaymentForm::Cash, 100)],
            None,
            BRAND,
            &capabilities(),
        )
        .unwrap();
        let mut journal = Recorder::default();
        let result = device.print_receipt(&receipt, &status, &mut journal);
        if refused {
            assert_eq!(
                result.unwrap_err().to_string(),
                "Nazwa usługi musi zawierać literę, cyfrę lub jeden ze znaków , . / \\ %.",
                "{name:?}"
            );
            assert_eq!(journal.stages, Vec::<&str>::new(), "{name:?}");
            assert_eq!(fiscal_commands(&sim), Vec::<String>::new(), "{name:?}");
        } else {
            let _printed = result.unwrap();
            assert_eq!(sim.receipts()[0].lines[0]["na"], name);
        }
    }
}

#[test]
fn a_refused_zero_daily_report_is_certain_and_other_report_codes_stay_uncertain() {
    let sim = simulator();
    sim.fault("dailyrep", Fault::Refused(382));
    let mut journal = Recorder::default();
    let error = printer(&sim)
        .print_report(Report::Daily, UNIQUE_NUMBER, &mut journal)
        .unwrap_err();
    assert!(
        matches!(&error, Error::Device { code: 382, rejected: false, command, .. }
            if command == "dailyrep"),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "Drukarka zwróciła błąd 382 (dailyrep). Nie można wykonać dwóch kolejnych zerowych \
         raportów dobowych."
    );
    assert_eq!(journal.stages, ["dailyrep"]);
    assert_eq!(fiscal_commands(&sim), ["dailyrep"], "never resent");
    assert_eq!(sim.reports(), vec![]);

    for (command, code, report) in [
        ("dailyrep", 2000, Report::Daily),
        (
            "monthlyrep",
            382,
            Report::Monthly {
                year: 2026,
                month: 6,
            },
        ),
    ] {
        let sim = simulator();
        sim.fault(command, Fault::Refused(code));
        let error = printer(&sim)
            .print_report(report, UNIQUE_NUMBER, &mut Recorder::default())
            .unwrap_err();
        assert!(
            matches!(error, Error::Uncertain(_)),
            "{command} {code}: {error:?}"
        );
    }
}

#[test]
fn a_read_only_printer_prints_monthly_and_periodic_reports_but_no_sales_or_daily_report() {
    let sim = simulator();
    sim.set_status("td", "1");
    let device = printer(&sim);
    let status = device.probe().unwrap();
    assert!(!status.ready());
    assert!(status.periodic_reports_allowed());
    assert!(!status.daily_report_allowed());
    let _verified = device.verify(UNIQUE_NUMBER).unwrap();

    let mut journal = Recorder::default();
    for error in [
        device
            .print_receipt(&sale(), &status, &mut journal)
            .unwrap_err(),
        device
            .print_report(Report::Daily, UNIQUE_NUMBER, &mut journal)
            .unwrap_err(),
    ] {
        assert!(
            matches!(&error, Error::NotReady(reason)
                if reason == "Drukarka jest w trybie tylko do odczytu"),
            "{error:?}"
        );
    }
    assert_eq!(journal.stages, Vec::<&str>::new());
    assert_eq!(fiscal_commands(&sim), Vec::<String>::new());

    for report in [
        Report::Monthly {
            year: 2026,
            month: 6,
        },
        Report::Periodic {
            from: date(2026, 6, 1),
            to: date(2026, 6, 30),
            summary: true,
        },
    ] {
        device
            .print_report(report, UNIQUE_NUMBER, &mut journal)
            .unwrap();
    }
    assert_eq!(journal.stages, ["monthlyrep", "periodicrepbydates"]);
    assert_eq!(
        sim.reports()
            .iter()
            .map(|report| report.command.as_str())
            .collect::<Vec<_>>(),
        ["monthlyrep", "periodicrepbydates"]
    );

    sim.set_status("pr", "5");
    let error = device
        .print_report(
            Report::Monthly {
                year: 2026,
                month: 5,
            },
            UNIQUE_NUMBER,
            &mut journal,
        )
        .unwrap_err();
    assert!(
        matches!(&error, Error::NotReady(reason)
            if reason == "Brak papieru. Drukarka jest w trybie tylko do odczytu"),
        "{error:?}"
    );
    assert_eq!(
        journal.stages,
        ["monthlyrep", "periodicrepbydates"],
        "other problems still refuse reports"
    );
}
