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
    simulator::{Fault, Simulator, UNIQUE_NUMBER},
    testing::{BRAND, line, pay},
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
fn an_unreadable_status_is_never_treated_as_a_ready_printer() {
    let sim = simulator();
    sim.fault(
        "sdev",
        Fault::Reply(vec![("ds".to_owned(), "0".to_owned())]),
    );
    assert_eq!(
        printer(&sim).probe().unwrap_err().to_string(),
        "Niepełna lub nieprawidłowa odpowiedź statusu drukarki: \
         Niepełna odpowiedź drukarki: brak parametru qe."
    );
    assert_eq!(fiscal_commands(&sim), Vec::<String>::new());
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
            "regraded" => sim.set_vat([
                "23,00", "8,00", "5,00", "0,00", "100,00", "101,00", "8,00",
            ]),
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
        let sim = simulator();
        sim.fault(
            command,
            Fault::Device {
                code: 2036,
                command: command.to_owned(),
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
        assert!(
            matches!(error, Error::Device { code: 2036, .. }),
            "{command}: {error:?}"
        );
        assert_eq!(journal.stages, [command]);
        assert_eq!(fiscal_commands(&sim), [command]);
        assert_eq!(sim.receipts(), vec![]);
        assert_eq!(sim.reports(), vec![]);
    }
    let sim = simulator();
    sim.fault(
        "dailyrep",
        Fault::Device {
            code: 2036,
            command: "!sdev".to_owned(),
            field: String::new(),
        },
    );
    assert!(
        matches!(
            printer(&sim).print_report(Report::Daily, UNIQUE_NUMBER, &mut Recorder::default()),
            Err(Error::Uncertain(_))
        ),
        "a foreign refusal proves nothing about dailyrep"
    );
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
    // Printer date 2026-07-01 (Warsaw); fiscalised 2026-03-10.
    for (report, prints) in [
        (
            Report::Monthly {
                year: 2026,
                month: 8,
            },
            false,
        ),
        (
            Report::Periodic {
                from: date(2026, 7, 2),
                to: date(2026, 7, 2),
                summary: false,
            },
            false,
        ),
        (
            Report::Monthly {
                year: 2026,
                month: 2,
            },
            false,
        ),
        (
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 3, 9),
                summary: true,
            },
            false,
        ),
        (
            Report::Monthly {
                year: 2026,
                month: 7,
            },
            true,
        ),
        (
            Report::Monthly {
                year: 2026,
                month: 3,
            },
            true,
        ),
        (
            Report::Periodic {
                from: date(2026, 7, 1),
                to: date(2026, 7, 1),
                summary: false,
            },
            true,
        ),
        (
            Report::Periodic {
                from: date(2026, 1, 1),
                to: date(2026, 3, 10),
                summary: true,
            },
            true,
        ),
    ] {
        let sim = simulator();
        sim.set_fiscalised("2026-03-10T08:00:00Z".parse().unwrap());
        let mut journal = Recorder::default();
        let result = printer(&sim).print_report(report, UNIQUE_NUMBER, &mut journal);
        if prints {
            assert!(result.is_ok(), "{report:?}: {result:?}");
            assert_eq!(journal.stages.len(), 1, "{report:?}");
            assert_eq!(sim.reports().len(), 1, "{report:?}");
        } else {
            assert!(
                matches!(&result, Err(Error::Invalid(_))),
                "{report:?}: {result:?}"
            );
            assert_eq!(journal.stages, Vec::<&str>::new(), "{report:?}");
            assert_eq!(fiscal_commands(&sim), Vec::<String>::new(), "{report:?}");
        }
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

macro_rules! uncertain_receipt {
    ($name:ident,$command:literal,$fault:expr,$kind:pat) => {
        #[test]
        fn $name() {
            let sim = simulator();
            sim.fault($command,$fault);
            let device = printer(&sim);
            let status = device.probe().unwrap();
            let mut journal = Recorder::default();
            let error = device
                .print_receipt(&all_steps_receipt(),&status,&mut journal)
                .unwrap_err();
            assert!(
                matches!(&error,Error::Uncertain(inner) if matches!(inner.as_ref(),$kind)),
                "{error:?}"
            );
            let expected = [
                "trinit","trnipset","trline","tradvance",
                "trpayment","trend","trftrln","trftrend"
            ];
            let end = expected.iter().position(|command|*command == $command).unwrap();
            assert_eq!(
                fiscal_commands(&sim),expected[..=end],
                "send each command only once and stop at the failure"
            );
            assert_eq!(journal.stages,expected[..=end],"journal every send before execution");
            assert_eq!(
                sim.transaction_open(),end < 5,
                "the command executed before its reply failed"
            );
            assert_eq!(sim.receipts().len(),usize::from(end >= 5));
        }
    };
}

macro_rules! receipt_faults {
    ($lost:ident,$corrupt:ident,$closed:ident,$command:literal) => {
        uncertain_receipt!($lost, $command, Fault::LoseReply, Error::Timeout(_));
        uncertain_receipt!($corrupt, $command, Fault::CorruptReply, Error::Protocol(_));
        uncertain_receipt!($closed, $command, Fault::Disconnect, Error::Io(_));
    };
}

receipt_faults!(lost_trinit, corrupt_trinit, disconnected_trinit, "trinit");
receipt_faults!(
    lost_trnipset,
    corrupt_trnipset,
    disconnected_trnipset,
    "trnipset"
);
receipt_faults!(lost_trline, corrupt_trline, disconnected_trline, "trline");
receipt_faults!(
    lost_tradvance,
    corrupt_tradvance,
    disconnected_tradvance,
    "tradvance"
);
receipt_faults!(
    lost_trpayment,
    corrupt_trpayment,
    disconnected_trpayment,
    "trpayment"
);
receipt_faults!(lost_trend, corrupt_trend, disconnected_trend, "trend");
receipt_faults!(
    lost_trftrln,
    corrupt_trftrln,
    disconnected_trftrln,
    "trftrln"
);
receipt_faults!(
    lost_trftrend,
    corrupt_trftrend,
    disconnected_trftrend,
    "trftrend"
);

macro_rules! report_faults {
    ($name:ident,$command:literal,$report:expr) => {
        #[test]
        fn $name() {
            for fault in [Fault::LoseReply, Fault::CorruptReply, Fault::Disconnect] {
                let sim = simulator();
                sim.fault($command, fault);
                let mut journal = Recorder::default();
                let error = printer(&sim)
                    .print_report($report, UNIQUE_NUMBER, &mut journal)
                    .unwrap_err();
                assert!(matches!(error, Error::Uncertain(_)), "{error:?}");
                assert_eq!(fiscal_commands(&sim), [$command], "never resend a report");
                assert_eq!(journal.stages, [$command]);
                assert_eq!(
                    sim.reports()[0].command,
                    $command,
                    "report executed before the reply failed"
                );
            }
        }
    };
}
report_faults!(daily_report_reply_failures, "dailyrep", Report::Daily);
report_faults!(
    monthly_report_reply_failures,
    "monthlyrep",
    Report::Monthly {
        year: 2026,
        month: 6
    }
);
report_faults!(
    periodic_report_reply_failures,
    "periodicrepbydates",
    Report::Periodic {
        from: date(2026, 1, 1),
        to: date(2026, 6, 30),
        summary: false
    }
);

#[test]
fn a_refused_later_receipt_step_is_uncertain_and_stops_the_stream() {
    for (index, command) in [
        "trnipset",
        "trline",
        "tradvance",
        "trpayment",
        "trend",
        "trftrln",
        "trftrend",
    ]
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
    let _printed = device
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
    assert_eq!(sim.receipts()[0].buyer_nip.as_deref(), Some("1234567890"));
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
        assert_eq!(sim.reports()[0].command, command);
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
            assert_eq!(sim.receipts().len(), 1);
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
        matches!(&error, Error::Device { code: 382, rejected: false, command, .. } if command == "dailyrep"),
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
            matches!(&error, Error::NotReady(reason) if reason == "Drukarka jest w trybie tylko do odczytu"),
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
        matches!(&error, Error::NotReady(reason) if reason.contains("Brak papieru")),
        "{error:?}"
    );
    assert_eq!(
        sim.reports().len(),
        2,
        "other problems still refuse reports"
    );
}
