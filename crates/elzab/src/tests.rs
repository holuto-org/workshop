#[cfg(not(workshop_acceptance))]
use crate::receipt as receipt_commands;
use crate::{
    frame,
    printer::{Printer, identify},
    profile,
    session::Session,
    simulator::{Fault, Simulator},
};
use fiscal::{
    cp1250,
    device::Vendor,
    driver::Driver,
    error::Error,
    journal::Journal,
    link::{Candidate, Connection, Link, Port},
    money::Grosze,
    receipt::{Advance, Line, Payment, PaymentForm, Quantity, Receipt, VatSlot},
    report::Report,
};
use std::{
    io,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
#[cfg(workshop_acceptance)]
mod acceptance;
mod telemetry;
#[derive(Default)]
struct Log {
    commands: Vec<&'static str>,
    fail_at: Option<usize>,
}
impl Journal for Log {
    fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
        if self.fail_at == Some(self.commands.len()) {
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.commands.push(command);
        Ok(())
    }

    fn waiting(&mut self, _: Option<&str>) {}
}
fn connection(sim: &Simulator) -> Connection {
    Connection {
        port: sim.address().to_owned(),
        baud_rate: 9600,
    }
}
fn printer(sim: &Simulator) -> Printer {
    Printer::new(sim.link(), connection(sim))
}
fn receipt(advance: u64, footer: bool) -> Receipt {
    let mut caps = profile::lookup("ELZAB CUBE online").unwrap();
    caps.zero_total_receipts = true;
    let lines = vec![
        Line::new(
            "Łożysko",
            Quantity::parse("1.23456").unwrap(),
            Grosze::new(10000).unwrap(),
            VatSlot::new(0).unwrap(),
            &caps,
        )
        .unwrap(),
    ];
    Receipt::new(
        lines,
        if advance == 0 {
            vec![]
        } else {
            vec![Advance {
                vat: VatSlot::new(0).unwrap(),
                amount: Grosze::new(advance).unwrap(),
            }]
        },
        vec![Payment {
            form: PaymentForm::Card,
            amount: Grosze::new(12346 - advance).unwrap(),
        }],
        Some("1234567890".to_owned()),
        if footer { "Inna firma" } else { "Warsztat" },
        &caps,
    )
    .unwrap()
}
fn bodies(sim: &Simulator) -> Vec<String> {
    sim.frames()
        .iter()
        .map(|raw| cp1250::decode(frame::decode(raw).unwrap().1).unwrap())
        .collect()
}
fn fiscal(sim: &Simulator) -> Vec<String> {
    bodies(sim)
        .into_iter()
        .filter(|b| {
            matches!(
                b.split('\u{1d}').next(),
                Some("LOR" | "LNP" | "LIT" | "LDS" | "LBP" | "LCL" | "LAL" | "RRP" | "RPP")
            )
        })
        .collect()
}

#[cfg(not(workshop_acceptance))]
#[test]
fn non_fiscal_device_is_not_ready_without_acceptance() {
    let sim = Simulator::start();
    sim.set_status(0, (1_u128 << 102) | (1_u128 << 98) | 4 | 8, 32);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.ready());
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut log),
        Err(Error::NotReady(_))
    ));
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), Vec::<String>::new());
}

#[test]
fn overdue_daily_report_blocks_receipts_but_allows_daily_report() {
    let sim = Simulator::start();
    sim.set_status(
        0,
        (1_u128 << 115) | (1_u128 << 102) | (1_u128 << 98) | 4,
        32,
    );
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.ready());
    assert!(expected.daily_report_allowed());
    assert_eq!(expected.description(), "Wymagany zaległy raport dobowy");
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut log),
        Err(Error::NotReady(_))
    ));
    assert!(matches!(
        driver.print_report(
            Report::Monthly {
                year: 2025,
                month: 9
            },
            &expected.unique_number,
            &mut log
        ),
        Err(Error::NotReady(_))
    ));
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), Vec::<String>::new());
    assert!(matches!(
        driver.print_report(Report::Daily, &expected.unique_number, &mut log),
        Err(Error::Uncertain(_))
    ));
    assert_eq!(log.commands, ["RRP"]);
    assert_eq!(fiscal(&sim), ["RRP\x1dI1\x1dJ1"]);
    assert_eq!(sim.reports(), 0, "acceptance is not completion");
    assert!(sim.daily_pending());
}

#[test]
fn read_only_printer_refuses_printing_but_allows_resolution() {
    let sim = Simulator::start();
    // Fiscal and idle, but package 2 writable bit 2 is clear.
    sim.set_status(0, (1_u128 << 102) | (1_u128 << 98), 32);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.ready());
    assert_eq!(
        expected.description(),
        "Drukarka jest w trybie tylko do odczytu"
    );
    // The driver refuses monthly/periodic reports below, so the UI must not offer them.
    assert!(!expected.periodic_reports_allowed());
    let _resolvable = driver.verify("ABC1234567890").unwrap();
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut log),
        Err(Error::NotReady(_))
    ));
    for report in [
        Report::Daily,
        Report::Monthly {
            year: 2026,
            month: 9,
        },
    ] {
        assert!(matches!(
            driver.print_report(report, "ABC1234567890", &mut log),
            Err(Error::NotReady(_))
        ));
    }
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), Vec::<String>::new());
}

#[cfg(not(workshop_acceptance))]
#[test]
fn unqualified_advance_zero_total_and_footer_are_refused_before_any_fiscal_command() {
    // LDS advances are qualified; only zero-total and footer need acceptance.
    let caps = profile::lookup("ELZAB CUBE online").unwrap();
    assert!(caps.advance_deductions);
    for (advance, footer, message) in [
        (
            12346,
            false,
            "Drukarka nie obsługuje paragonu opłaconego w całości zaliczkami.",
        ),
        (0, true, FOOTER_REFUSAL),
    ] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(advance, footer), &expected, &mut log),
            Err(Error::Invalid(text)) if text == message
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
}
/// Names the fix: the printer header must contain the Holuto company name.
const FOOTER_REFUSAL: &str = "Nagłówek drukarki nie zawiera nazwy firmy z Holuto, a ta drukarka \
                              nie drukuje jej w stopce. Wpisz w Holuto nazwę zgodną z nagłówkiem \
                              drukarki.";
#[test]
fn identify_accepts_online_models_only() {
    for model in [
        "ELZAB ZETA ONLINE",
        "ELZAB MERA ONLINE",
        "ELZAB CUBE online",
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, "MD06.02");
        let device = identify(sim.link().as_ref(), &connection(&sim))
            .unwrap()
            .unwrap();
        assert!(device.ready(), "{}", device.description());
        assert_eq!(device.unique_number, "ABC1234567890");
    }
    for model in [
        "ELZAB ZETA",
        "ELZAB MERA+",
        "ELZAB Zeta Online",
        "ELZAB UNKNOWN ONLINE",
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, "MD06.02");
        assert!(matches!(
            identify(sim.link().as_ref(), &connection(&sim)),
            Err(Error::Unsupported(message))
                if message.contains(model) && message.contains("MD06.02")
        ));
        assert_eq!(bodies(&sim), ["XTR"], "nothing beyond identification");
    }
    let sim = Simulator::start();
    sim.set_identity("POSNET TEMO ONLINE", "MD06.02");
    assert!(
        identify(sim.link().as_ref(), &connection(&sim))
            .unwrap()
            .is_none()
    );
    assert_eq!(bodies(&sim), ["XTR"]);
}
#[test]
fn non_cp1250_code_page_is_not_ready() {
    for page in ["CP852", "MAZOVIA"] {
        let sim = Simulator::start();
        sim.set_code_page(page);
        let device = printer(&sim).probe().unwrap();
        assert!(!device.ready());
        assert_eq!(device.capabilities.allowed_characters, "");
    }
    let sim = Simulator::start();
    let device = printer(&sim).probe().unwrap();
    assert!(device.capabilities.allows("Łódź €"));
    assert!(!device.capabilities.allows("😀\n"));
}
#[test]
fn transport_status_rejection_of_first_frame_is_certain() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.fault("LOR", Fault::Transport("SL".to_owned()));
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut Log::default()),
        Err(Error::Device { rejected: true, .. })
    ));
    assert_eq!(fiscal(&sim), ["LOR\x1dC0"]);
    assert_eq!(sim.receipts(), 0);
}
#[test]
fn reply_with_foreign_command_id_is_ignored() {
    let sim = Simulator::start();
    sim.fault("XTR", Fault::ForeignReply);
    let mut session = Session::open(sim.link().as_ref(), &connection(&sim)).unwrap();
    assert!(
        session
            .command("XTR", "XTR", Duration::from_secs(3))
            .unwrap()
            .contains("ELZAB CUBE online")
    );
    assert_eq!(sim.frames().len(), 1);
}
#[test]
fn receipt_frames_match_snapshot() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let mut log = Log::default();
    let printed = driver
        .print_receipt(&receipt(2346, false), &expected, &mut log)
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(log.commands, ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL"]);
    insta::assert_debug_snapshot!(fiscal(&sim), @r#"
    [
        "LOR\u{1d}C0",
        "LNP\u{1d}OA1234567890",
        "LIT\u{1d}GAŁożysko\u{1e}BA\u{1e}C1,23456\u{1e}D100,00\u{1e}E123,46",
        "LDS\u{1d}IAA1\u{1f}B23,46\u{1f}DZaliczka\u{1e}CA\u{1e}D",
        "LBP\u{1d}KA1\u{1e}BKARTA\u{1e}C100,00\u{1e}X2",
        "LCL\u{1d}RA100,00",
    ]
    "#);
}
#[test]
fn paper_out_mid_receipt_is_uncertain() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.fault("LIT", Fault::PaperOut);
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut Log::default()),
        Err(Error::Uncertain(_))
    ));
    assert_eq!(sim.receipts(), 0);
    assert!(!sim.transaction_open());
    assert_eq!(fiscal(&sim).len(), 3);
}
#[test]
fn daily_report_acceptance_is_unknown_and_blocks() {
    let sim = Simulator::start();
    let mut log = Log::default();
    assert!(matches!(
        printer(&sim).print_report(Report::Daily, "ABC1234567890", &mut log),
        Err(Error::Uncertain(_))
    ));
    assert_eq!(log.commands, ["RRP"]);
    assert_eq!(sim.reports(), 0);
    assert_eq!(fiscal(&sim), ["RRP\x1dI1\x1dJ1"]);
}
#[cfg(not(workshop_acceptance))]
#[test]
fn fully_prepaid_receipt_is_refused_before_open() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(matches!(
        driver.print_receipt(&receipt(12346, false), &expected, &mut Log::default()),
        Err(Error::Invalid(_))
    ));
    assert_eq!(fiscal(&sim), Vec::<String>::new());
}
#[test]
fn fiscal_faults_are_never_retried() {
    for command in ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL"] {
        for fault in [
            Fault::LoseReply,
            Fault::CorruptReply,
            Fault::Disconnect,
            Fault::Device(450),
        ] {
            let sim = Simulator::start();
            let driver = printer(&sim);
            let expected = driver.probe().unwrap();
            sim.fault(command, fault);
            assert!(matches!(
                driver.print_receipt(&receipt(2346, false), &expected, &mut Log::default()),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(
                fiscal(&sim)
                    .iter()
                    .filter(|b| b.starts_with(command))
                    .count(),
                1
            );
        }
    }
}
#[test]
fn journal_failure_prevents_next_send() {
    for index in 0..6 {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut log = Log {
            fail_at: Some(index),
            ..Log::default()
        };
        let error = driver
            .print_receipt(&receipt(2346, false), &expected, &mut log)
            .unwrap_err();
        assert!(if index == 0 {
            matches!(error, Error::Journal(_))
        } else {
            matches!(error, Error::Uncertain(_))
        });
        assert_eq!(fiscal(&sim).len(), index);
    }
}
#[test]
fn fresh_identity_vat_status_and_header_are_required() {
    // The acceptance build deliberately permits the non-fiscal-mode case.
    for change in (0..4).filter(|&change| !cfg!(workshop_acceptance) || change != 2) {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        match change {
            0 => sim.set_unique_number("OTHER"),
            1 => sim.set_vat(["22,00", "8,00", "5,00", "0,00", "=", "?", "?"]),
            2 => sim.set_status(0, (1_u128 << 102) | (1_u128 << 98) | 4 | 8, 32),
            _ => sim.set_header(""),
        }
        assert!(
            driver
                .print_receipt(&receipt(0, false), &expected, &mut Log::default())
                .is_err(),
            "case {change}"
        );
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
}
#[test]
fn legal_vat_change_since_probe_never_opens() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    // Slot G may hold any rate, so the device stays ready; only the VAT guard refuses.
    sim.set_vat(["23,00", "8,00", "5,00", "0,00", "=", "?", "8,00"]);
    assert!(driver.probe().unwrap().ready());
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut log),
        Err(Error::Invalid(message))
            if message == "Drukarka lub stawki VAT zmieniły się. Odśwież połączenie."
    ));
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), Vec::<String>::new());
}
#[test]
fn wrong_vendor_or_inactive_vat_slot_never_opens() {
    let caps = profile::lookup("ELZAB CUBE online").unwrap();
    let inactive = Receipt::new(
        vec![
            Line::new(
                "Olej",
                Quantity::parse("1").unwrap(),
                Grosze::new(1000).unwrap(),
                // The simulator programs F as "?".
                VatSlot::new(5).unwrap(),
                &caps,
            )
            .unwrap(),
        ],
        vec![],
        vec![Payment {
            form: PaymentForm::Cash,
            amount: Grosze::new(1000).unwrap(),
        }],
        None,
        "Warsztat",
        &caps,
    )
    .unwrap();
    for (vendor, receipt, message) in [
        (
            Vendor::Posnet,
            receipt(0, false),
            "Drukarka lub stawki VAT zmieniły się. Odśwież połączenie.",
        ),
        (
            Vendor::Elzab,
            inactive,
            "Wybrana stawka VAT jest nieaktywna.",
        ),
    ] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let mut expected = driver.probe().unwrap();
        expected.vendor = vendor;
        assert!(matches!(
            driver.print_receipt(&receipt, &expected, &mut Log::default()),
            Err(Error::Invalid(actual)) if actual == message
        ));
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
}
#[test]
fn nested_lpc_refusal_prevents_open_while_status_reports_ready() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.set_receipt_permission(1203);
    assert!(driver.probe().unwrap().ready());
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut log),
        Err(Error::NotReady(message)) if message.contains("1203")
    ));
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), Vec::<String>::new());
    sim.set_receipt_permission(0);
    let _printed = driver
        .print_receipt(&receipt(0, false), &expected, &mut Log::default())
        .unwrap();
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn every_document_and_mechanism_blocker_prevents_dispatch_and_names_its_cause() {
    const SERVICE: &str =
        "Drukarka Elzab wymaga serwisu: pamięć, inicjalizacja lub blokada sprzedaży";
    const POWER: &str = "Sprawdź zasilanie i akumulator drukarki Elzab";
    const DISPLAY: &str = "Podłącz wyświetlacz klienta drukarki Elzab";
    const OPERATOR: &str =
        "Drukarka Elzab jest w menu lub teście albo wydruk przerwano na drukarce";
    const PRINTING: &str = "Drukarka Elzab jeszcze drukuje. Poczekaj na koniec wydruku";
    const DOCUMENT: &str = "Otwarty dokument na drukarce Elzab. Dokończ lub anuluj go na drukarce";
    const TRAINING: &str = "Drukarka Elzab jest w trybie szkoleniowym";
    const PAPER: &str = "Brak papieru lub podniesiona głowica drukarki Elzab";
    const MECHANISM: &str = "Awaria mechanizmu drukującego Elzab";
    let ready = [0, (1_u128 << 102) | (1_u128 << 98) | 4, 32];
    let set = [
        (0, 123, SERVICE),
        (0, 122, SERVICE),
        (0, 115, POWER),
        (0, 103, POWER),
        (0, 102, POWER),
        (1, 122, POWER),
        (1, 101, DISPLAY),
        (1, 100, SERVICE),
        (1, 99, SERVICE),
        (1, 90, SERVICE),
        (1, 87, SERVICE),
        (1, 82, OPERATOR),
        (1, 81, PRINTING),
        (1, 78, SERVICE),
        (1, 77, POWER),
        (1, 70, DOCUMENT),
        (1, 68, DOCUMENT),
        (1, 67, OPERATOR),
        (1, 66, DOCUMENT),
        (1, 65, DOCUMENT),
        (1, 64, OPERATOR),
        (1, 3, TRAINING),
        (1, 0, SERVICE),
        (2, 4, PAPER),
        (2, 3, PAPER),
        (2, 2, MECHANISM),
        (2, 1, PAPER),
        (2, 0, PRINTING),
    ]
    .into_iter()
    .filter(|&(package, bit, _)| !cfg!(workshop_acceptance) || (package, bit) != (1, 3))
    .map(|(package, bit, message)| {
        let mut status = ready;
        *status.get_mut(package).unwrap() |= 1_u128 << bit;
        (status, (package, bit), message)
    });
    let cleared = [
        (1, 102, POWER),
        (1, 98, SERVICE),
        (1, 2, "Drukarka jest w trybie tylko do odczytu"),
        (2, 5, MECHANISM),
    ]
    .map(|(package, bit, message)| {
        let mut status = ready;
        *status.get_mut(package).unwrap() &= !(1_u128 << bit);
        (status, (package, bit), message)
    });
    for ([one, two, three], (package, bit), message) in set.chain(cleared) {
        let sim = Simulator::start();
        sim.set_status(one, two, u32::try_from(three).unwrap());
        let device = printer(&sim).probe().unwrap();
        assert!(!device.ready(), "package {package} bit {bit}");
        assert_eq!(device.description(), message, "package {package} bit {bit}");
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
    let sim = Simulator::start();
    sim.set_status(0, ready[1] | (1 << 68) | (1 << 67), 32 | 2 | 8);
    assert_eq!(
        printer(&sim).probe().unwrap().description(),
        format!("{OPERATOR}. {DOCUMENT}. {PAPER}"),
        "each cause is listed once, in a stable order"
    );
}

#[test]
fn monthly_and_periodic_reports_are_explicitly_synchronous() {
    use jiff::civil::date;
    let sim = Simulator::start();
    let driver = printer(&sim);
    for report in [
        Report::Monthly {
            year: 2026,
            month: 9,
        },
        Report::Periodic {
            from: date(2026, 9, 1),
            to: date(2026, 9, 30),
            summary: false,
        },
        Report::Periodic {
            from: date(2026, 9, 1),
            to: date(2026, 9, 30),
            summary: true,
        },
    ] {
        driver
            .print_report(report, "ABC1234567890", &mut Log::default())
            .unwrap();
    }
    assert_eq!(
        fiscal(&sim),
        [
            "RPP\x1dE1\x1dFB2609\x1dS0",
            "RPP\x1dE1\x1dFE260901\x1eF260930\x1dS0",
            "RPP\x1dE3\x1dFE260901\x1eF260930\x1dS0"
        ]
    );
    assert_eq!(sim.reports(), 3);
}

#[test]
fn periodic_report_past_printer_date_or_inverted_never_dispatches() {
    use jiff::civil::date;
    let sim = Simulator::start();
    let driver = printer(&sim);
    let periodic = |from, to| Report::Periodic {
        from,
        to,
        summary: false,
    };
    // The simulated printer date is 2026-10-24.
    for report in [
        periodic(date(2026, 10, 1), date(2026, 10, 25)),
        periodic(date(2026, 10, 20), date(2026, 10, 19)),
    ] {
        let mut log = Log::default();
        assert!(matches!(
            driver.print_report(report, "ABC1234567890", &mut log),
            Err(Error::Invalid(message)) if message == "Nieprawidłowy zakres dat raportu."
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
    }
    assert_eq!(fiscal(&sim), Vec::<String>::new());
    driver
        .print_report(
            periodic(date(2026, 10, 24), date(2026, 10, 24)),
            "ABC1234567890",
            &mut Log::default(),
        )
        .unwrap();
    assert_eq!(fiscal(&sim), ["RPP\x1dE1\x1dFE261024\x1eF261024\x1dS0"]);
}

#[test]
fn monthly_report_of_the_printer_month_never_dispatches() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let monthly = |month| Report::Monthly { year: 2026, month };
    // Printer dates 2026-10-24 and 2026-10-31: October has not ended on the printer.
    for clock in ["261024120000", "261031235959"] {
        sim.set_clock(clock);
        for month in [10, 11] {
            let mut log = Log::default();
            assert!(matches!(
                driver.print_report(monthly(month), "ABC1234567890", &mut log),
                Err(Error::Invalid(message))
                    if message == "Raport miesięczny wymaga zakończonego miesiąca."
            ));
            assert_eq!(log.commands, Vec::<&str>::new());
        }
    }
    assert_eq!(fiscal(&sim), Vec::<String>::new());
    driver
        .print_report(monthly(9), "ABC1234567890", &mut Log::default())
        .unwrap();
    assert_eq!(fiscal(&sim), ["RPP\x1dE1\x1dFB2609\x1dS0"]);
}

#[test]
fn accepted_daily_report_stays_pending_until_a_later_connection_has_seen_it() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    assert!(matches!(
        driver.print_report(Report::Daily, "ABC1234567890", &mut Log::default()),
        Err(Error::Uncertain(_))
    ));
    assert!(sim.daily_pending());
    assert!(matches!(
        driver.print_report(Report::Daily, "ABC1234567890", &mut Log::default()),
        Err(Error::NotReady(_))
    ));
    // The printout finished; the operator can now resolve the interrupted report.
    let _resolvable = driver.verify("ABC1234567890").unwrap();
    assert!(!sim.daily_pending());
    assert_eq!(fiscal(&sim).len(), 1);
}

#[test]
fn dst_fold_clock_blocks_only_receipt_timestamps() {
    use jiff::civil::date;
    let sim = Simulator::start();
    sim.set_clock("261025023000");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(expected.ready(), "{}", expected.description());
    driver
        .print_report(
            Report::Periodic {
                from: date(2026, 9, 1),
                to: date(2026, 9, 30),
                summary: false,
            },
            "ABC1234567890",
            &mut Log::default(),
        )
        .unwrap();
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(0, false), &expected, &mut log),
        Err(Error::Protocol(_))
    ));
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), ["RPP\x1dE1\x1dFE260901\x1eF260930\x1dS0"]);
}

#[test]
fn missing_number_never_replays_a_completed_receipt() {
    struct NumberFault<'a> {
        sim: &'a Simulator,
    }
    impl Journal for NumberFault<'_> {
        fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
            if command == "LCL" {
                self.sim.fault("SER", Fault::LoseReply);
            }
            Ok(())
        }

        fn waiting(&mut self, _: Option<&str>) {}
    }
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let printed = driver
        .print_receipt(
            &receipt(0, false),
            &expected,
            &mut NumberFault { sim: &sim },
        )
        .unwrap();
    assert_eq!(printed.receipt_number, None);
    assert_eq!(sim.receipts(), 1);
    assert_eq!(
        fiscal(&sim).iter().filter(|b| b.starts_with("LCL")).count(),
        1
    );
}

#[test]
fn fiscal_intent_is_persisted_before_each_frame() {
    struct Ordering<'a> {
        sim: &'a Simulator,
        recorded: usize,
    }
    impl Journal for Ordering<'_> {
        fn dispatching(&mut self, _: &'static str) -> io::Result<()> {
            if fiscal(self.sim).len() != self.recorded {
                return Err(io::Error::other("fiscal frame preceded its journal intent"));
            }
            self.recorded += 1;
            Ok(())
        }

        fn waiting(&mut self, _: Option<&str>) {}
    }
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let _printed = driver
        .print_receipt(
            &receipt(2346, false),
            &expected,
            &mut Ordering {
                sim: &sim,
                recorded: 0,
            },
        )
        .unwrap();
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn configured_header_all_lines_are_used_without_writing_configuration() {
    let sim = Simulator::start();
    sim.set_header("Serwis Łódź\nWARSZTAT");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let _printed = driver
        .print_receipt(&receipt(0, false), &expected, &mut Log::default())
        .unwrap();
    assert!(!fiscal(&sim).iter().any(|b| b.starts_with("LAL")));
    assert!(bodies(&sim).iter().any(|b| b == "yHR\x1dva2"));
    assert!(!bodies(&sim).iter().any(|b| b.ends_with('W')));
}

#[cfg(not(workshop_acceptance))]
#[test]
fn brand_across_header_lines_needs_no_footer() {
    for (header, printed) in [("Serwis Inna\nfirma", true), ("Innafirma", false)] {
        let sim = Simulator::start();
        sim.set_header(header);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let result = driver.print_receipt(&receipt(0, true), &expected, &mut Log::default());
        if printed {
            assert!(result.is_ok(), "{header:?}: {result:?}");
            assert!(!fiscal(&sim).iter().any(|b| b.starts_with("LAL")));
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(
                matches!(&result, Err(Error::Invalid(text)) if text == FOOTER_REFUSAL),
                "{header:?}: {result:?}"
            );
            assert_eq!(fiscal(&sim), Vec::<String>::new());
        }
    }
}

#[test]
fn every_payment_form_uses_online_kind_not_entry_number() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let caps = &expected.capabilities;
    let line = Line::new(
        "Olej",
        Quantity::parse("1").unwrap(),
        Grosze::new(500).unwrap(),
        VatSlot::new(0).unwrap(),
        caps,
    )
    .unwrap();
    let payments = [
        PaymentForm::Cash,
        PaymentForm::Card,
        PaymentForm::Transfer,
        PaymentForm::Mobile,
        PaymentForm::Credit,
    ]
    .map(|form| Payment {
        form,
        amount: Grosze::new(100).unwrap(),
    })
    .to_vec();
    let receipt = Receipt::new(vec![line], vec![], payments, None, "Warsztat", caps).unwrap();
    let _printed = driver
        .print_receipt(&receipt, &expected, &mut Log::default())
        .unwrap();
    assert_eq!(
        fiscal(&sim)
            .into_iter()
            .filter(|b| b.starts_with("LBP"))
            .collect::<Vec<_>>(),
        [
            "LBP\x1dKA1\x1eBGOTÓWKA\x1eC1,00\x1eX1",
            "LBP\x1dKA2\x1eBKARTA\x1eC1,00\x1eX2",
            "LBP\x1dKA3\x1eBPRZELEW\x1eC1,00\x1eX7",
            "LBP\x1dKA4\x1eBMOBILNA\x1eC1,00\x1eX9",
            "LBP\x1dKA5\x1eBKREDYT\x1eC1,00\x1eX6",
        ]
    );
}

#[test]
fn receipt_is_revalidated_against_actual_name_and_quantity_limits() {
    for (name, quantity) in [("Ł".repeat(65), "1"), ("Olej".to_owned(), "1.000001")] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut permissive = expected.capabilities.clone();
        permissive.max_item_name = 80;
        permissive.max_quantity_decimals = 8;
        let line = Line::new(
            &name,
            Quantity::parse(quantity).unwrap(),
            Grosze::new(100).unwrap(),
            VatSlot::new(0).unwrap(),
            &permissive,
        )
        .unwrap();
        let receipt = Receipt::new(
            vec![line],
            vec![],
            vec![Payment {
                form: PaymentForm::Cash,
                amount: Grosze::new(100).unwrap(),
            }],
            None,
            "Warsztat",
            &permissive,
        )
        .unwrap();
        assert!(matches!(
            driver.print_receipt(&receipt, &expected, &mut Log::default()),
            Err(Error::Invalid(_))
        ));
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
}

#[test]
fn exact_64_character_names_print_without_utf8_truncation() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let name = "Ł".repeat(64);
    let line = Line::new(
        &name,
        Quantity::parse("1").unwrap(),
        Grosze::new(100).unwrap(),
        VatSlot::new(0).unwrap(),
        &expected.capabilities,
    )
    .unwrap();
    let receipt = Receipt::new(
        vec![line],
        vec![],
        vec![Payment {
            form: PaymentForm::Cash,
            amount: Grosze::new(100).unwrap(),
        }],
        None,
        "Warsztat",
        &expected.capabilities,
    )
    .unwrap();
    let _printed = driver
        .print_receipt(&receipt, &expected, &mut Log::default())
        .unwrap();
    assert!(
        fiscal(&sim)
            .iter()
            .any(|body| body == &format!("LIT\x1dGA{name}\x1eBA\x1eC1\x1eD1,00\x1eE1,00"))
    );
}

#[test]
fn close_effect_survives_lost_corrupt_or_disconnected_reply() {
    for fault in [Fault::LoseReply, Fault::CorruptReply, Fault::Disconnect] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault("LCL", fault);
        assert!(matches!(
            driver.print_receipt(&receipt(0, false), &expected, &mut Log::default()),
            Err(Error::Uncertain(_))
        ));
        assert_eq!(sim.receipts(), 1);
        assert!(!sim.transaction_open());
        assert_eq!(
            fiscal(&sim).iter().filter(|b| b.starts_with("LCL")).count(),
            1
        );
    }
}

#[test]
fn report_failures_and_transport_rejections_remain_uncertain() {
    use jiff::civil::date;
    for report in [
        Report::Daily,
        Report::Monthly {
            year: 2026,
            month: 9,
        },
        Report::Periodic {
            from: date(2026, 9, 1),
            to: date(2026, 9, 30),
            summary: true,
        },
    ] {
        for fault in [
            Fault::LoseReply,
            Fault::CorruptReply,
            Fault::Disconnect,
            Fault::Device(503),
            Fault::Transport("SL".to_owned()),
        ] {
            let sim = Simulator::start();
            sim.fault(
                if report == Report::Daily {
                    "RRP"
                } else {
                    "RPP"
                },
                fault,
            );
            assert!(matches!(
                printer(&sim).print_report(report, "ABC1234567890", &mut Log::default()),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(fiscal(&sim).len(), 1);
        }
    }
}

#[cfg(not(workshop_acceptance))]
#[test]
fn unqualified_footer_is_refused_and_qualified_profile_emits_lal() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(matches!(
        driver.print_receipt(&receipt(0, true), &expected, &mut Log::default()),
        Err(Error::Invalid(text)) if text == FOOTER_REFUSAL
    ));
    assert_eq!(fiscal(&sim), Vec::<String>::new());
    let mut qualified = profile::lookup("ELZAB CUBE online").unwrap();
    qualified.company_footer = true;
    let steps = receipt_commands::steps(&receipt(0, true), Some("Inna firma"), &qualified).unwrap();
    assert_eq!(steps.last().unwrap().body, "LAL\x1dQAAInna firma");
    assert_eq!(steps.last().unwrap().command, "LAL");
}

#[test]
fn simulator_link_is_exclusive_and_never_opens_another_port() {
    let sim = Simulator::start();
    let link = sim.link();
    assert!(matches!(
        link.open("not-a-simulator", 9600),
        Err(Error::Open { source, .. }) if source.kind() == io::ErrorKind::NotFound
    ));
    let first = link.open(sim.address(), 9600).unwrap();
    assert!(matches!(
        link.open(sim.address(), 9600),
        Err(Error::Open { source, .. }) if source.kind() == io::ErrorKind::PermissionDenied
    ));
    drop(first);
    drop(link.open(sim.address(), 9600).unwrap());
    assert_eq!(link.time_scale(), 1000);
}

#[test]
fn inconsistent_multipart_lcl_reply_is_uncertain_not_printed() {
    struct CorruptCloseTotal<'a> {
        sim: &'a Simulator,
    }
    impl Journal for CorruptCloseTotal<'_> {
        fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
            if command == "LCL" {
                let frames = self.sim.frames();
                let raw = frames.last().ok_or(io::ErrorKind::InvalidData)?;
                let (header, _) = frame::decode(raw).map_err(io::Error::other)?;
                let id = header.parse::<u32>().map_err(io::Error::other)? + 1;
                let mut reply = frame::encode(&format!("{id}\x1dSA\x1d1\x1d3"), b"")
                    .map_err(io::Error::other)?;
                reply.extend(
                    frame::encode(&format!("{id}\x1dSA\x1d2\x1d2"), b"")
                        .map_err(io::Error::other)?,
                );
                self.sim.fault("LCL", Fault::Reply(reply));
            }
            Ok(())
        }

        fn waiting(&mut self, _: Option<&str>) {}
    }
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let error = driver
        .print_receipt(
            &receipt(0, false),
            &expected,
            &mut CorruptCloseTotal { sim: &sim },
        )
        .unwrap_err();
    assert!(matches!(error, Error::Uncertain(cause) if matches!(*cause, Error::Protocol(_))));
    assert_eq!(sim.receipts(), 1);
    assert_eq!(
        fiscal(&sim).iter().filter(|b| b.starts_with("LCL")).count(),
        1
    );
}
/// Command, write budget and the first read budget after it.
type Budgets = Arc<Mutex<Vec<(String, Duration, Option<Duration>)>>>;
/// Real-time link: records each command's write budget and the read budget that follows it.
#[derive(Debug)]
struct Deadlines {
    inner: Arc<dyn Link>,
    seen: Budgets,
}
impl Link for Deadlines {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Ok(Box::new(DeadlinePort {
            inner: self.inner.open(port, baud_rate)?,
            seen: Arc::clone(&self.seen),
        }))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.inner.candidates()
    }

    fn time_scale(&self) -> u32 {
        1
    }
}
struct DeadlinePort {
    inner: Box<dyn Port>,
    seen: Budgets,
}
impl Port for DeadlinePort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        let (_, data) = frame::decode(bytes).map_err(io::Error::other)?;
        let body = cp1250::decode(data).ok_or(io::ErrorKind::InvalidData)?;
        let command = body.split('\u{1d}').next().unwrap_or_default().to_owned();
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((command, timeout, None));
        self.inner.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if let Some((_, _, read @ None)) = self
            .seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last_mut()
        {
            *read = Some(timeout);
        }
        self.inner.read(buffer, timeout)
    }
}

#[test]
fn silence_budget_is_short_for_receipt_lines_and_long_only_for_synchronous_reports() {
    let sim = Simulator::start();
    let seen = Arc::default();
    let driver = Printer::new(
        Arc::new(Deadlines {
            inner: sim.link(),
            seen: Arc::clone(&seen),
        }),
        connection(&sim),
    );
    let expected = driver.probe().unwrap();
    let _printed = driver
        .print_receipt(&receipt(2346, false), &expected, &mut Log::default())
        .unwrap();
    driver
        .print_report(
            Report::Monthly {
                year: 2026,
                month: 9,
            },
            &expected.unique_number,
            &mut Log::default(),
        )
        .unwrap();
    assert!(matches!(
        driver.print_report(Report::Daily, &expected.unique_number, &mut Log::default()),
        Err(Error::Uncertain(_))
    ));
    let line = Duration::from_secs(10);
    let end = Duration::from_secs(30);
    let report = Duration::from_secs(600);
    let fiscal: Vec<_> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(command, ..)| {
            ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL", "RPP", "RRP"].contains(&command.as_str())
        })
        .cloned()
        .collect();
    assert_eq!(
        fiscal
            .iter()
            .map(|(command, write, _)| (command.as_str(), *write))
            .collect::<Vec<_>>(),
        [
            ("LOR", line),
            ("LNP", line),
            ("LIT", line),
            ("LDS", line),
            ("LBP", line),
            ("LCL", end),
            ("RPP", report),
            ("RRP", end),
        ]
    );
    for (command, write, read) in fiscal {
        let read = read.unwrap();
        assert!(
            write
                .checked_sub(read)
                .is_some_and(|spent| spent < Duration::from_secs(1)),
            "{command}: read budget {read:?} must be the remaining {write:?}"
        );
    }
}
