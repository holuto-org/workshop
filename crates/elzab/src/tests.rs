use crate::{
    frame,
    printer::{Printer, identify},
    profile,
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
use jiff::civil::date;
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
fn refused_daily_report_lists_the_overdue_report_with_other_blockers() {
    let sim = Simulator::start();
    // Overdue daily report on a read-only printer: package 2 writable bit 2 is clear.
    sim.set_status(0, (1_u128 << 115) | (1_u128 << 102) | (1_u128 << 98), 32);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.daily_report_allowed());
    let mut log = Log::default();
    let error = driver
        .print_report(Report::Daily, &expected.unique_number, &mut log)
        .unwrap_err();
    assert!(
        matches!(&error, Error::NotReady(text) if *text == expected.description()
            && text.contains("Wymagany zaległy raport dobowy")),
        "{error:?}"
    );
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal(&sim), Vec::<String>::new());
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
#[cfg(not(workshop_acceptance))]
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
            Err(Error::Unsupported(message)) if message == format!(
                "Nieobsługiwany model drukarki Elzab: {model} (oprogramowanie MD06.02). \
                 Obsługiwane są Elzab Zeta, Mera i Cube ONLINE."
            )
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
fn receipt_frames_match_snapshot() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let mut log = Log::default();
    let printed = driver
        .print_receipt(&receipt(2346, false), &expected, &mut log)
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(printed.issued_at.to_string(), "2026-10-24T10:00:00Z");
    assert_eq!(sim.receipts(), 1);
    assert!(!sim.transaction_open());
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
fn fiscal_faults_are_never_retried() {
    let commands = ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL"];
    let frames = [
        "LOR\x1dC0",
        "LNP\x1dOA1234567890",
        "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
        "LDS\x1dIAA1\x1fB23,46\x1fDZaliczka\x1eCA\x1eD",
        "LBP\x1dKA1\x1eBKARTA\x1eC100,00\x1eX2",
        "LCL\x1dRA100,00",
    ];
    for (index, command) in commands.iter().enumerate() {
        for fault in [
            Fault::LoseReply,
            Fault::CorruptReply,
            Fault::Disconnect,
            Fault::Device(450),
            Fault::Transport("SL".to_owned()),
            Fault::Reply(frame::encode("999999999", b"").unwrap()),
        ] {
            let sim = Simulator::start();
            let driver = printer(&sim);
            let expected = driver.probe().unwrap();
            let executes = !matches!(fault, Fault::Device(_) | Fault::Transport(_));
            let certain = index == 0 && matches!(fault, Fault::Transport(_));
            sim.fault(command, fault);
            let mut log = Log::default();
            let error = driver
                .print_receipt(&receipt(2346, false), &expected, &mut log)
                .unwrap_err();
            assert!(if certain {
                matches!(
                    error,
                    Error::Device {
                        rejected: true,
                        code: 76,
                        ..
                    }
                )
            } else {
                matches!(error, Error::Uncertain(_))
            });
            assert_eq!(fiscal(&sim), frames[..=index]);
            assert_eq!(log.commands, commands[..=index]);
            assert_eq!(sim.receipts(), u32::from(index == 5 && executes));
            assert_eq!(
                sim.transaction_open(),
                (index != 0 || executes) && !(index == 5 && executes)
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
        let frames = [
            "LOR\x1dC0",
            "LNP\x1dOA1234567890",
            "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
            "LDS\x1dIAA1\x1fB23,46\x1fDZaliczka\x1eCA\x1eD",
            "LBP\x1dKA1\x1eBKARTA\x1eC100,00\x1eX2",
            "LCL\x1dRA100,00",
        ];
        assert_eq!(fiscal(&sim), frames[..index]);
        assert_eq!(
            log.commands,
            ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL"][..index]
        );
        assert_eq!(sim.receipts(), 0);
        assert_eq!(sim.transaction_open(), index > 0);
    }
}
#[test]
fn identity_header_and_code_page_are_rechecked_before_dispatch() {
    for change in 0..3 {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        match change {
            0 => sim.set_unique_number("OTHER"),
            1 => sim.set_header(""),
            _ => sim.set_code_page("CP852"),
        }
        let mut log = Log::default();
        let error = driver
            .print_receipt(&receipt(0, false), &expected, &mut log)
            .unwrap_err();
        match change {
            0 => assert!(matches!(error, Error::NotReady(_))),
            1 => assert!(matches!(error, Error::NotReady(message)
                if message == "Brak zaprogramowanego nagłówka lub NIP")),
            _ => assert!(matches!(error, Error::NotReady(message)
                if message == "Ustaw stronę kodową drukarki na CP1250")),
        }
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
        assert_eq!(
            (sim.receipts(), sim.reports(), sim.transaction_open()),
            (0, 0, false)
        );
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
        Err(Error::NotReady(message)) if message == "Elzab nie pozwala otworzyć paragonu: 1203."
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
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.set_status(one, two, u32::try_from(three).unwrap());
        let device = driver.probe().unwrap();
        assert!(!device.ready(), "package {package} bit {bit}");
        assert_eq!(device.description(), message, "package {package} bit {bit}");
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(0, false), &expected, &mut log),
            Err(Error::NotReady(actual)) if actual == message
        ));
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
            assert!(matches!(
                driver.print_report(report, &expected.unique_number, &mut log),
                Err(Error::NotReady(actual)) if actual == message
            ));
        }
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
        assert_eq!(
            (sim.receipts(), sim.reports(), sim.transaction_open()),
            (0, 0, false)
        );
    }
}

#[test]
fn simultaneous_status_causes_are_deduplicated_in_operator_order() {
    let sim = Simulator::start();
    let ready = (1_u128 << 102) | (1_u128 << 98) | 4;
    sim.set_status(0, ready | (1 << 68) | (1 << 67), 32 | 2 | 8);
    assert_eq!(
        printer(&sim).probe().unwrap().description(),
        "Drukarka Elzab jest w menu lub teście albo wydruk przerwano na drukarce. \
         Otwarty dokument na drukarce Elzab. Dokończ lub anuluj go na drukarce. \
         Brak papieru lub podniesiona głowica drukarki Elzab",
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
    let _resolvable = driver.verify("ABC1234567890").unwrap();
    assert!(!sim.daily_pending());
    assert_eq!(fiscal(&sim), ["RRP\x1dI1\x1dJ1"]);
}

#[test]
fn dst_fold_and_gap_clocks_block_only_receipt_timestamps() {
    for clock in ["261025023000", "260329023000"] {
        let sim = Simulator::start();
        sim.set_clock(clock);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(expected.ready(), "{}", expected.description());
        assert_eq!(
            driver.verify("ABC1234567890").unwrap().unique_number,
            "ABC1234567890"
        );
        driver
            .print_report(
                Report::Periodic {
                    from: date(2026, 2, 1),
                    to: date(2026, 2, 28),
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
        assert_eq!(fiscal(&sim), ["RPP\x1dE1\x1dFE260201\x1eF260228\x1dS0"]);
        assert_eq!(sim.receipts(), 0);
    }
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
        fiscal(&sim),
        [
            "LOR\x1dC0",
            "LNP\x1dOA1234567890",
            "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
            "LBP\x1dKA1\x1eBKARTA\x1eC123,46\x1eX2",
            "LCL\x1dRA123,46",
        ]
    );
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
    assert_eq!(
        bodies(&sim),
        [
            "XTR",
            "FNR",
            "XHR\x1dw",
            "ySR",
            "YYR\x1dJ",
            "yHR\x1dva1",
            "yHR\x1dva2",
            "yHR\x1dx",
            "FTR",
            "XCR",
            "XTR",
            "FNR",
            "XHR\x1dw",
            "ySR",
            "YYR\x1dJ",
            "yHR\x1dva1",
            "yHR\x1dva2",
            "yHR\x1dx",
            "FTR",
            "XCR",
            "LPC\x1dA1",
            "SER",
            "RNR",
            "XCR",
            "LOR\x1dC0",
            "LNP\x1dOA1234567890",
            "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
            "LBP\x1dKA1\x1eBKARTA\x1eC123,46\x1eX2",
            "LCL\x1dRA123,46",
            "SER",
        ]
    );
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
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            assert_eq!(
                fiscal(&sim),
                [
                    "LOR\x1dC0",
                    "LNP\x1dOA1234567890",
                    "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
                    "LBP\x1dKA1\x1eBKARTA\x1eC123,46\x1eX2",
                    "LCL\x1dRA123,46",
                ]
            );
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
            Err(Error::Invalid(message))
                if message == "Pozycja nie mieści się w profilu Elzab (64 znaki, 5 miejsc ilości)."
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
fn report_failures_and_transport_rejections_remain_uncertain() {
    for (report, command, body) in [
        (Report::Daily, "RRP", "RRP\x1dI1\x1dJ1"),
        (
            Report::Monthly {
                year: 2026,
                month: 9,
            },
            "RPP",
            "RPP\x1dE1\x1dFB2609\x1dS0",
        ),
        (
            Report::Periodic {
                from: date(2026, 9, 1),
                to: date(2026, 9, 30),
                summary: true,
            },
            "RPP",
            "RPP\x1dE3\x1dFE260901\x1eF260930\x1dS0",
        ),
    ] {
        for fault in [
            Fault::LoseReply,
            Fault::CorruptReply,
            Fault::Disconnect,
            Fault::Device(503),
            Fault::Transport("SL".to_owned()),
            Fault::Reply(frame::encode("999999999", b"").unwrap()),
        ] {
            let sim = Simulator::start();
            let executes = !matches!(fault, Fault::Device(_) | Fault::Transport(_));
            sim.fault(command, fault);
            let mut log = Log::default();
            assert!(matches!(
                printer(&sim).print_report(report, "ABC1234567890", &mut log),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(log.commands, [command]);
            assert_eq!(fiscal(&sim), [body]);
            assert_eq!(
                sim.reports(),
                u32::from(executes && report != Report::Daily)
            );
            assert_eq!(sim.daily_pending(), executes && report == Report::Daily);
            assert_eq!((sim.receipts(), sim.transaction_open()), (0, false));
        }
    }
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
}

#[test]
fn inconsistent_multipart_close_is_uncertain_despite_the_completed_effect() {
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
        fiscal(&sim),
        [
            "LOR\x1dC0",
            "LNP\x1dOA1234567890",
            "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
            "LBP\x1dKA1\x1eBKARTA\x1eC123,46\x1eX2",
            "LCL\x1dRA123,46",
        ]
    );
}
/// Command, write budget and the first read budget after it.
type Budgets = Arc<Mutex<Vec<(String, Duration, Option<Duration>)>>>;
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
            !read.is_zero() && read <= write,
            "{command}: read budget {read:?} must be the remaining {write:?}"
        );
    }
}

#[test]
fn malformed_preflight_fields_never_reach_a_fiscal_command() {
    for (command, id, body) in [
        (
            "XTR",
            "1",
            "\x1dBELZAB CUBE online\x1dBELZAB ZETA ONLINE\x1dDMD06.02",
        ),
        ("XTR", "1", "\x1dBELZAB CUBE online"),
        ("FNR", "2", "\x1dB"),
        ("FNR", "2", "\x1dBABC 1234567890"),
        ("XHR", "3", "\x1dwACP1250\x1dwACP1250"),
        ("ySR", "4", "\x1dSA1\x1eB0\x1dSA2\x1eB4"),
        ("ySR", "4", "\x1dSA1\x1eB0\x1dSA1\x1eB0\x1dSA3\x1eB20"),
        ("ySR", "4", "\x1dSA0\x1eB0\x1dSA2\x1eB4\x1dSA3\x1eB20"),
        ("ySR", "4", "\x1dSA1\x1eBG\x1dSA2\x1eB4\x1dSA3\x1eB20"),
        (
            "ySR",
            "4",
            "\x1dSA1\x1eB0\x1dSA2\x1eB4\x1dSA3\x1eB100000000",
        ),
        (
            "ySR",
            "4",
            "\x1dSA1\x1eB000000000000000000000000000000000\x1dSA2\x1eB4\x1dSA3\x1eB20",
        ),
        ("YYR", "5", "\x1dJA0"),
        ("YYR", "5", "\x1dJA257"),
        ("yHR", "6", "\x1dva1\x1ebWarsztat\x1ebDuplicate"),
        (
            "FTR",
            "8",
            "\x1dAA23,00\x1eB8,00\x1eC5,00\x1eD0,00\x1eE=\x1eF?",
        ),
        (
            "FTR",
            "8",
            "\x1dAA23,00\x1eA8,00\x1eB8,00\x1eC5,00\x1eD0,00\x1eE=\x1eF?\x1eG?",
        ),
        (
            "FTR",
            "8",
            "\x1dAA23,0\x1eB8,00\x1eC5,00\x1eD0,00\x1eE=\x1eF?\x1eG?",
        ),
        (
            "FTR",
            "8",
            "\x1dAA23,00\x1eB8,00\x1eC5,00\x1eD0,00\x1eE=\x1eF101,00\x1eG?",
        ),
        (
            "FTR",
            "8",
            "\x1dAA23,00\x1eB8,00\x1eC5,00\x1eD0,00\x1eE=\x1eF-\x1eG?",
        ),
        ("XCR", "9", "\x1dA26102412000"),
        ("XCR", "9", "\x1dA2610241200000"),
        ("XCR", "9", "\x1dA26102412000:"),
        ("SER", "11", "\x1dJnot-a-number"),
        ("SER", "11", "\x1dJ123456789012345678901"),
        ("RNR", "12", "\x1dAinvalid"),
        ("XCR", "9", "\x1dA500101120000"),
        ("XCR", "9", "\x1dA260229120000"),
        ("XCR", "9", "\x1dA261024240000"),
    ] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault(
            command,
            Fault::Reply(frame::encode(id, body.as_bytes()).unwrap()),
        );
        let mut log = Log::default();
        assert!(
            matches!(
                driver.print_receipt(&receipt(0, false), &expected, &mut log),
                Err(Error::Protocol(message)) if message == "Nieprawidłowa ramka ElzabSTX."
            ),
            "{command}: {body:?}"
        );
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
        assert_eq!(
            (sim.receipts(), sim.reports(), sim.transaction_open()),
            (0, 0, false)
        );
    }
}

#[test]
fn reports_require_the_same_serial_and_a_successful_journal_before_dispatch() {
    for report in [
        Report::Daily,
        Report::Monthly {
            year: 2026,
            month: 9,
        },
        Report::Periodic {
            from: date(2026, 9, 1),
            to: date(2026, 9, 30),
            summary: false,
        },
    ] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let mut log = Log::default();
        assert!(matches!(
            driver.print_report(report, "OTHER", &mut log),
            Err(Error::Invalid(message)) if message ==
                "Podłączona drukarka ABC1234567890 nie jest drukarką OTHER, \
                 której dotyczy operacja."
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        log.fail_at = Some(0);
        assert!(
            matches!(driver.print_report(report, "ABC1234567890", &mut log),
            Err(Error::Journal(error)) if error.kind() == io::ErrorKind::StorageFull)
        );
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
        assert_eq!(
            (sim.receipts(), sim.reports(), sim.daily_pending()),
            (0, 0, false)
        );
    }
}

#[test]
fn report_year_and_month_boundaries_refuse_before_journaling() {
    for report in [
        Report::Monthly {
            year: 1999,
            month: 12,
        },
        Report::Monthly {
            year: 2026,
            month: 0,
        },
        Report::Monthly {
            year: 2026,
            month: 13,
        },
        Report::Periodic {
            from: date(1999, 12, 31),
            to: date(2000, 1, 1),
            summary: false,
        },
    ] {
        let sim = Simulator::start();
        let mut log = Log::default();
        assert!(matches!(
            printer(&sim).print_report(report, "ABC1234567890", &mut log),
            Err(Error::Invalid(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
        assert_eq!(sim.reports(), 0);
    }
}

#[test]
fn printer_civil_clock_epoch_and_offsets_determine_issue_time() {
    for (clock, expected_time) in [
        ("000101120000", "2000-01-01T11:00:00Z"),
        ("490101120000", "2049-01-01T11:00:00Z"),
        ("260701120000", "2026-07-01T10:00:00Z"),
        ("261201120000", "2026-12-01T11:00:00Z"),
    ] {
        let sim = Simulator::start();
        sim.set_clock(clock);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let printed = driver
            .print_receipt(&receipt(0, false), &expected, &mut Log::default())
            .unwrap();
        assert_eq!(printed.issued_at.to_string(), expected_time);
        assert_eq!(printed.receipt_number.as_deref(), Some("1"));
        assert_eq!(sim.receipts(), 1);
    }
}

#[test]
fn receipt_line_limit_is_rechecked_before_lor() {
    for lines in [255, 256] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut permissive = expected.capabilities.clone();
        permissive.max_lines = 256;
        let line = Line::new(
            "Olej",
            Quantity::parse("1").unwrap(),
            Grosze::new(1).unwrap(),
            VatSlot::new(0).unwrap(),
            &permissive,
        )
        .unwrap();
        let receipt = Receipt::new(
            vec![line; lines],
            vec![],
            vec![Payment {
                form: PaymentForm::Cash,
                amount: Grosze::new(u64::try_from(lines).unwrap()).unwrap(),
            }],
            None,
            "Warsztat",
            &permissive,
        )
        .unwrap();
        let mut log = Log::default();
        let result = driver.print_receipt(&receipt, &expected, &mut log);
        if lines == 255 {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            let mut frames = vec!["LOR\x1dC0".to_owned()];
            frames.extend(vec![
                "LIT\x1dGAOlej\x1eBA\x1eC1\x1eD0,01\x1eE0,01".to_owned();
                255
            ]);
            frames.extend([
                "LBP\x1dKA1\x1eBGOTÓWKA\x1eC2,55\x1eX1".to_owned(),
                "LCL\x1dRA2,55".to_owned(),
            ]);
            assert_eq!(fiscal(&sim), frames);
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(matches!(result, Err(Error::Invalid(message))
                if message == "Dane paragonu przekraczają możliwości Elzab."));
            assert_eq!(log.commands, Vec::<&str>::new());
            assert_eq!(fiscal(&sim), Vec::<String>::new());
            assert_eq!(sim.receipts(), 0);
        }
    }
}

#[test]
fn seven_nonzero_payments_are_allowed_but_an_eighth_prevents_lor() {
    for count in [7, 8] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let caps = &expected.capabilities;
        let line = Line::new(
            "Olej",
            Quantity::parse("1").unwrap(),
            Grosze::new(count).unwrap(),
            VatSlot::new(0).unwrap(),
            caps,
        )
        .unwrap();
        let mut payments = vec![Payment {
            form: PaymentForm::Cash,
            amount: Grosze::ZERO,
        }];
        payments.extend(vec![
            Payment {
                form: PaymentForm::Card,
                amount: Grosze::new(1).unwrap()
            };
            usize::try_from(count).unwrap()
        ]);
        let receipt = Receipt::new(vec![line], vec![], payments, None, "Warsztat", caps).unwrap();
        let mut log = Log::default();
        let result = driver.print_receipt(&receipt, &expected, &mut log);
        if count == 7 {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            assert_eq!(
                fiscal(&sim),
                [
                    "LOR\x1dC0",
                    "LIT\x1dGAOlej\x1eBA\x1eC1\x1eD0,07\x1eE0,07",
                    "LBP\x1dKA1\x1eBKARTA\x1eC0,01\x1eX2",
                    "LBP\x1dKA2\x1eBKARTA\x1eC0,01\x1eX2",
                    "LBP\x1dKA3\x1eBKARTA\x1eC0,01\x1eX2",
                    "LBP\x1dKA4\x1eBKARTA\x1eC0,01\x1eX2",
                    "LBP\x1dKA5\x1eBKARTA\x1eC0,01\x1eX2",
                    "LBP\x1dKA6\x1eBKARTA\x1eC0,01\x1eX2",
                    "LBP\x1dKA7\x1eBKARTA\x1eC0,01\x1eX2",
                    "LCL\x1dRA0,07",
                ]
            );
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(matches!(result, Err(Error::Invalid(message))
                if message == "Elzab przyjmuje najwyżej 7 wpisów płatności."));
            assert_eq!(log.commands, Vec::<&str>::new());
            assert_eq!(fiscal(&sim), Vec::<String>::new());
            assert_eq!(sim.receipts(), 0);
        }
    }
}

#[test]
fn mixed_vat_advances_keep_their_native_slots_and_exact_fiscal_total() {
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let caps = &expected.capabilities;
    let lines = (0..5)
        .map(|slot| {
            Line::new(
                "Olej",
                Quantity::parse("1").unwrap(),
                Grosze::new(1000).unwrap(),
                VatSlot::new(slot).unwrap(),
                caps,
            )
            .unwrap()
        })
        .collect();
    let advances = [(0, 100), (1, 200), (4, 300)]
        .map(|(slot, amount)| Advance {
            vat: VatSlot::new(slot).unwrap(),
            amount: Grosze::new(amount).unwrap(),
        })
        .to_vec();
    let receipt = Receipt::new(
        lines,
        advances,
        vec![Payment {
            form: PaymentForm::Transfer,
            amount: Grosze::new(4400).unwrap(),
        }],
        None,
        "Warsztat",
        caps,
    )
    .unwrap();
    let mut log = Log::default();
    assert_eq!(
        driver
            .print_receipt(&receipt, &expected, &mut log)
            .unwrap()
            .receipt_number
            .as_deref(),
        Some("1")
    );
    assert_eq!(
        fiscal(&sim),
        [
            "LOR\x1dC0",
            "LIT\x1dGAOlej\x1eBA\x1eC1\x1eD10,00\x1eE10,00",
            "LIT\x1dGAOlej\x1eBB\x1eC1\x1eD10,00\x1eE10,00",
            "LIT\x1dGAOlej\x1eBC\x1eC1\x1eD10,00\x1eE10,00",
            "LIT\x1dGAOlej\x1eBD\x1eC1\x1eD10,00\x1eE10,00",
            "LIT\x1dGAOlej\x1eBE\x1eC1\x1eD10,00\x1eE10,00",
            "LDS\x1dIAA1\x1fB1,00\x1fDZaliczka\x1eCA\x1eD",
            "LDS\x1dIAA1\x1fB2,00\x1fDZaliczka\x1eCB\x1eD",
            "LDS\x1dIAA1\x1fB3,00\x1fDZaliczka\x1eCE\x1eD",
            "LBP\x1dKA1\x1eBPRZELEW\x1eC44,00\x1eX7",
            "LCL\x1dRA44,00",
        ]
    );
    assert_eq!(
        log.commands,
        [
            "LOR", "LIT", "LIT", "LIT", "LIT", "LIT", "LDS", "LDS", "LDS", "LBP", "LCL"
        ]
    );
    assert_eq!((sim.receipts(), sim.transaction_open()), (1, false));
}

#[test]
fn status_packages_accept_their_full_declared_widths() {
    for (third, description) in [
        ("00000020", "Gotowa do drukowania"),
        (
            "FFFFFFFF",
            "Drukarka Elzab jeszcze drukuje. Poczekaj na koniec wydruku. \
            Brak papieru lub podniesiona głowica drukarki Elzab. \
            Awaria mechanizmu drukującego Elzab",
        ),
    ] {
        let sim = Simulator::start();
        let two = (1_u128 << 102) | (1_u128 << 98) | 4;
        let body = format!(
            "\x1dSA1\x1eB{:032X}\x1dSA2\x1eB{two:032X}\x1dSA3\x1eB{third}",
            0
        );
        sim.fault(
            "ySR",
            Fault::Reply(frame::encode("4", body.as_bytes()).unwrap()),
        );
        let device = printer(&sim).probe().unwrap();
        assert_eq!(device.description(), description);
        assert_eq!(device.ready(), third == "00000020");
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
}

#[test]
fn every_permitted_header_line_is_read_before_opening() {
    let sim = Simulator::start();
    let mut header = vec!["Serwis"; 255];
    header.push("Warsztat");
    sim.set_header(&header.join("\n"));
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let printed = driver
        .print_receipt(&receipt(0, false), &expected, &mut Log::default())
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(
        fiscal(&sim),
        [
            "LOR\x1dC0",
            "LNP\x1dOA1234567890",
            "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
            "LBP\x1dKA1\x1eBKARTA\x1eC123,46\x1eX2",
            "LCL\x1dRA123,46",
        ]
    );
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn company_and_buyer_nip_widths_are_revalidated_without_truncation() {
    for (brand_length, nip_length) in [(42, 42), (43, 42), (42, 43)] {
        let sim = Simulator::start();
        let brand = "Ł".repeat(brand_length);
        let nip = "1".repeat(nip_length);
        sim.set_header(&brand);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut permissive = expected.capabilities.clone();
        permissive.max_company_name = 64;
        permissive.max_buyer_nip = 64;
        let line = Line::new(
            "Olej",
            Quantity::parse("1").unwrap(),
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
            Some(nip.clone()),
            &brand,
            &permissive,
        )
        .unwrap();
        let mut log = Log::default();
        let result = driver.print_receipt(&receipt, &expected, &mut log);
        if brand_length == 42 && nip_length == 42 {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            assert_eq!(
                fiscal(&sim),
                [
                    "LOR\x1dC0".to_owned(),
                    format!("LNP\x1dOA{nip}"),
                    "LIT\x1dGAOlej\x1eBA\x1eC1\x1eD1,00\x1eE1,00".to_owned(),
                    "LBP\x1dKA1\x1eBGOTÓWKA\x1eC1,00\x1eX1".to_owned(),
                    "LCL\x1dRA1,00".to_owned(),
                ]
            );
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(matches!(result, Err(Error::Invalid(message))
                if message == "Dane paragonu przekraczają możliwości Elzab."));
            assert_eq!(log.commands, Vec::<&str>::new());
            assert_eq!(fiscal(&sim), Vec::<String>::new());
            assert_eq!((sim.receipts(), sim.transaction_open()), (0, false));
        }
    }
}

#[test]
fn twenty_digit_printer_counters_are_not_rejected_or_reformatted() {
    struct Counters<'a>(&'a Simulator);
    impl Journal for Counters<'_> {
        fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
            if command == "LCL" {
                let frames = self.0.frames();
                let raw = frames.last().ok_or(io::ErrorKind::InvalidData)?;
                let (header, _) = frame::decode(raw).map_err(io::Error::other)?;
                let id = header.parse::<u32>().map_err(io::Error::other)? + 2;
                self.0.fault(
                    "SER",
                    Fault::Reply(
                        frame::encode(&id.to_string(), b"\x1dJ12345678901234567890")
                            .map_err(io::Error::other)?,
                    ),
                );
            }
            Ok(())
        }

        fn waiting(&mut self, _: Option<&str>) {}
    }
    let sim = Simulator::start();
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.fault(
        "RNR",
        Fault::Reply(frame::encode("12", b"\x1dA12345678901234567890").unwrap()),
    );
    let printed = driver
        .print_receipt(&receipt(0, false), &expected, &mut Counters(&sim))
        .unwrap();
    assert_eq!(
        printed.receipt_number.as_deref(),
        Some("12345678901234567890")
    );
    assert_eq!((sim.receipts(), sim.transaction_open()), (1, false));
}
