use std::{io, sync::Arc, time::Duration};

use fiscal::{
    cp1250,
    device::VatKind,
    driver::Driver,
    error::Error,
    journal::Journal,
    link::{Candidate, Connection, Link, Port},
    money::Grosze,
    receipt::{Advance, Line, Payment, PaymentForm, Quantity, Receipt, VatSlot},
    report::Report,
};

use super::{Printer, identify};
use crate::{
    frame, profile,
    session::{DLE2, identifier},
    simulator::{Fault, Simulator},
};
#[cfg(workshop_acceptance)]
mod acceptance;
mod telemetry;

#[derive(Default)]
struct Log {
    commands: Vec<&'static str>,
    fail_at: Option<usize>,
    waits: Vec<Option<String>>,
}
impl Journal for Log {
    fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
        if self.fail_at == Some(self.commands.len()) {
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.commands.push(command);
        Ok(())
    }

    fn waiting(&mut self, problem: Option<&str>) {
        self.waits.push(problem.map(str::to_owned));
    }
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
fn supported(sim: &Simulator) {
    sim.set_identity("NOVITUS HD II ONLINE", "1.40");
}
fn receipt(advances: Vec<Advance>) -> Receipt {
    let caps = profile::lookup("NOVITUS HD II ONLINE", "1.40").unwrap();
    let lines = [
        ("Naprawa", 1000, 0),
        ("Olej", 2000, 1),
        ("Przeglad", 3000, 4),
    ]
    .map(|(name, price, slot)| {
        Line::new(
            name,
            Quantity::parse("1").unwrap(),
            Grosze::new(price).unwrap(),
            VatSlot::new(slot).unwrap(),
            &caps,
        )
        .unwrap()
    })
    .to_vec();
    let deducted: u64 = advances.iter().map(|a| a.amount.get()).sum();
    let payments = if deducted == 0 {
        [
            PaymentForm::Cash,
            PaymentForm::Card,
            PaymentForm::Transfer,
            PaymentForm::Mobile,
            PaymentForm::Credit,
        ]
        .map(|form| Payment {
            form,
            amount: Grosze::new(1200).unwrap(),
        })
        .to_vec()
    } else {
        vec![Payment {
            form: PaymentForm::Cash,
            amount: Grosze::new(6000 - deducted).unwrap(),
        }]
    };
    Receipt::new(
        lines,
        advances,
        payments,
        Some("1234567890".to_owned()),
        "Warsztat",
        &caps,
    )
    .unwrap()
}
fn bodies(sim: &Simulator) -> Vec<String> {
    sim.frames()
        .iter()
        .filter(|raw| raw.starts_with(&frame::START))
        .map(|raw| {
            let payload = frame::decode(raw).or_else(|_| frame::unframe(raw)).unwrap();
            cp1250::decode(payload).unwrap()
        })
        .collect()
}
fn fiscal_bodies(sim: &Simulator) -> Vec<String> {
    bodies(sim)
        .into_iter()
        .filter(|body| {
            matches!(
                identifier(body),
                Some("$h" | "$l" | "@z" | "$b" | "$y" | "$z" | "#r" | "#o")
            )
        })
        .collect()
}

const RECEIPT_FRAMES: [&str; 13] = [
    "0$h",
    "100;1$z1234567890\r",
    "1$lNaprawa\r1\rA/10/10/",
    "2$lOlej\r1\rB/20/20/",
    "3$lPrzeglad\r1\rE/30/30/",
    "1;0$b12/\r",
    "1;1$b12/\r",
    "1;8$b12/\r",
    "1;9$b12/\r",
    "1;5$b12/\r",
    "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r60/0/0/60/0/",
    "12$z",
    "28;0$z",
];

const RECEIPT_COMMANDS: [&str; 13] = [
    "$h", "$z", "$l", "$l", "$l", "$b", "$b", "$b", "$b", "$b", "$y", "$z", "$z",
];

#[cfg(not(workshop_acceptance))]
#[test]
fn non_fiscal_device_is_not_ready_without_acceptance() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_fiscal_mode(0);
    sim.set_status(0x74, 0x65, "@s0;0;0;00000000;");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.ready());
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&receipt(vec![]), &expected, &mut log),
        Err(Error::NotReady(_))
    ));
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
}

#[test]
fn read_only_printer_refuses_printing_but_allows_resolution() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_fiscal_mode(2);
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
        driver.print_receipt(&receipt(vec![]), &expected, &mut log),
        Err(Error::NotReady(_))
    ));
    for report in [
        Report::Daily,
        Report::Monthly {
            year: 2024,
            month: 2,
        },
        Report::Periodic {
            from: "2024-01-01".parse().unwrap(),
            to: "2024-01-31".parse().unwrap(),
            summary: true,
        },
    ] {
        assert!(matches!(
            driver.print_report(report, "ABC1234567890", &mut log),
            Err(Error::NotReady(message)) if message == expected.description()
        ));
    }
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
}

#[cfg(not(workshop_acceptance))]
#[test]
fn unqualified_advance_zero_total_and_footer_are_refused_before_any_fiscal_command() {
    for case in 0..3 {
        let sim = Simulator::start();
        supported(&sim);
        let (advances, message) = match case {
            0 => (
                vec![Advance {
                    vat: VatSlot::new(0).unwrap(),
                    amount: Grosze::new(500).unwrap(),
                }],
                ADVANCE_REFUSAL,
            ),
            1 => (
                [(0, 1000), (1, 2000), (4, 3000)]
                    .map(|(slot, amount)| Advance {
                        vat: VatSlot::new(slot).unwrap(),
                        amount: Grosze::new(amount).unwrap(),
                    })
                    .to_vec(),
                ADVANCE_REFUSAL,
            ),
            _ => {
                sim.set_header("Inny podatnik");
                (vec![], FOOTER_REFUSAL)
            },
        };
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(advances), &expected, &mut log),
            Err(Error::Invalid(text)) if text == message
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new(), "case {case}");
    }
}
#[cfg(not(workshop_acceptance))]
const ADVANCE_REFUSAL: &str = "Drukarka nie obsługuje rozliczenia zaliczek.";
/// Names the fix: the printer header must contain the Holuto company name.
#[cfg(not(workshop_acceptance))]
const FOOTER_REFUSAL: &str = "Nagłówek drukarki nie zawiera nazwy firmy z Holuto, a ta drukarka \
                              nie drukuje jej w stopce. Wpisz w Holuto nazwę zgodną z nagłówkiem \
                              drukarki.";

#[test]
fn identify_accepts_supported_models_only() {
    for (model, firmware, name_width, company_width) in [
        ("NOVITUS HD II ONLINE", "1.40", 60, 40),
        ("NOVITUS HD II ONLINE", "2.20", 60, 40),
        ("NOVITUS HD II ONLINE", "3.50", 60, 40),
        ("NOVITUS DEON ONLINE", "1.40", 40, 42),
        ("NOVITUS DEON ONLINE", "220", 60, 42),
        ("NOVITUS DEON ONLINE", "310", 60, 42),
        ("NOVITUS BONO ONLINE", "1.10", 40, 42),
        ("NOVITUS BONO ONLINE", "210", 60, 42),
        ("NOVITUS BONO ONLINE", "300", 60, 42),
        ("NOVITUS POINT", "1.0", 60, 44),
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, firmware);
        let device = identify(sim.link().as_ref(), &connection(&sim))
            .unwrap()
            .unwrap();
        assert!(device.ready(), "{}", device.description());
        assert_eq!(device.unique_number, "ABC1234567890");
        assert_eq!(device.model, model);
        assert_eq!(device.firmware, firmware);
        assert_eq!(device.capabilities.max_item_name, name_width);
        assert_eq!(device.capabilities.max_company_name, company_width);
        assert_eq!(
            (
                device.capabilities.advance_deductions,
                device.capabilities.zero_total_receipts,
                device.capabilities.company_footer,
            ),
            (
                cfg!(workshop_acceptance),
                cfg!(workshop_acceptance),
                cfg!(workshop_acceptance)
            )
        );
    }
    for (model, firmware) in [
        ("POSNET TEMO ONLINE", "1.40"),
        ("NOVITUS UNKNOWN", "1.40"),
        ("NOVITUS HD II ONLINE", "99.00"),
        ("NOVITUS HD II ONLINE", "3.50 "),
        ("NOVITUS HD II ONLINE", "3.51"),
        ("NOVITUS DEON ONLINE", "2.20"),
        ("NOVITUS BONO ONLINE", "2.10"),
        ("NOVITUS POINT", "1.00"),
        ("NOVITUS POINT ONLINE", "1.0"),
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, firmware);
        let found = identify(sim.link().as_ref(), &connection(&sim)).unwrap();
        if model.starts_with("NOVITUS ") {
            let device = found.unwrap();
            assert_eq!(device.description(), "Nieznany model lub firmware Novitus");
            let mut log = Log::default();
            assert!(matches!(
                printer(&sim).print_receipt(&receipt(vec![]), &device, &mut log),
                Err(Error::NotReady(message)) if message == device.description()
            ));
            assert_eq!(log.commands, Vec::<&str>::new());
        } else {
            assert!(found.is_none());
            assert_eq!(bodies(&sim), ["30#v"]);
        }
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn pc2_endpoint_is_not_ready_and_receives_no_fiscal_command() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_endpoint("PC2");
    assert!(
        identify(sim.link().as_ref(), &connection(&sim))
            .unwrap()
            .is_none()
    );
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(!expected.ready());
    assert!(matches!(
        driver.print_receipt(&receipt(vec![]), &expected, &mut Log::default()),
        Err(Error::NotReady(_))
    ));
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    assert!(
        !bodies(&sim).iter().any(|body| body.contains("^c")),
        "PC2 must be rejected before PC1-only reads"
    );
}

#[test]
fn vat_sentinels_map_exempt_and_inactive() {
    for vat in [
        ["23", "8", "5", "0", "98.99", "99.99", "99.99"],
        ["23", "8", "5", "0", "100", "101", "101"],
    ] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_vat(vat);
        let device = printer(&sim).probe().unwrap();
        assert_eq!(
            device.vat.iter().map(|v| v.kind).collect::<Vec<_>>(),
            [
                VatKind::Rate { basis_points: 2300 },
                VatKind::Rate { basis_points: 800 },
                VatKind::Rate { basis_points: 500 },
                VatKind::Rate { basis_points: 0 },
                VatKind::Exempt,
                VatKind::Inactive,
                VatKind::Inactive
            ]
        );
    }
}

#[test]
fn receipt_frames_are_exact() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let mut log = Log::default();
    let printed = driver
        .print_receipt(&receipt(vec![]), &expected, &mut log)
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(printed.issued_at.to_string(), "2026-10-24T10:00:00Z");
    assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES);
    assert_eq!(log.commands, RECEIPT_COMMANDS);
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn only_report_refusals_documented_before_printing_are_certain() {
    let periodic = Report::Periodic {
        from: "2024-01-02".parse().unwrap(),
        to: "2024-03-04".parse().unwrap(),
        summary: false,
    };
    for (report, body, code, certain) in [
        (Report::Daily, "1;26;10;24#r", 37, true),
        (Report::Daily, "1;26;10;24#r", 36, true),
        (Report::Daily, "1;26;10;24#r", 35, false),
        (Report::Daily, "1;26;10;24#r", 6, false),
        (Report::Daily, "1;26;10;24#r", 4, false),
        (Report::Daily, "1;26;10;24#r", 2, false),
        (periodic, "24;1;2;24;3;4;0#o", 4, true),
        (
            Report::Monthly {
                year: 2024,
                month: 2,
            },
            "24;2;1;24;2;29;6#o",
            4,
            true,
        ),
        (periodic, "24;1;2;24;3;4;0#o", 6, false),
        (periodic, "24;1;2;24;3;4;0#o", 37, false),
        (periodic, "24;1;2;24;3;4;0#o", 36, false),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        sim.fault(body, Fault::Refuse(code));
        let error = printer(&sim)
            .print_report(report, "ABC1234567890", &mut Log::default())
            .unwrap_err();
        if certain {
            assert!(
                matches!(error, Error::Device { code: refused, .. } if refused == code),
                "{body} {code}: {error:?}"
            );
        } else {
            assert!(matches!(error, Error::Uncertain(_)), "{body} {code}");
        }
        assert_eq!(fiscal_bodies(&sim), [body]);
    }
}

#[test]
fn certain_refusals_explain_documented_codes() {
    let receipt_refusal = |code| {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault("$h", Fault::Refuse(code));
        driver
            .print_receipt(&receipt(vec![]), &expected, &mut Log::default())
            .unwrap_err()
    };
    let report_refusal = |report, body, code| {
        let sim = Simulator::start();
        supported(&sim);
        sim.fault(body, Fault::Refuse(code));
        printer(&sim)
            .print_report(report, "ABC1234567890", &mut Log::default())
            .unwrap_err()
    };
    let periodic = Report::Periodic {
        from: "2024-01-02".parse().unwrap(),
        to: "2024-03-04".parse().unwrap(),
        summary: false,
    };
    for (error, text) in [
        (
            report_refusal(Report::Daily, "1;26;10;24#r", 36),
            "Drukarka Novitus zwróciła błąd 36 (#r). Raport dobowy bez sprzedaży jest już \
             zapisany z tą datą.",
        ),
        (
            report_refusal(Report::Daily, "1;26;10;24#r", 37),
            "Drukarka Novitus zwróciła błąd 37 (#r). Raport przerwano na klawiaturze drukarki.",
        ),
        (
            report_refusal(periodic, "24;1;2;24;3;4;0#o", 4),
            "Drukarka Novitus zwróciła błąd 4 (#o). Nieprawidłowy zakres dat lub brak zapisów \
             w pamięci fiskalnej dla tego zakresu.",
        ),
        (
            receipt_refusal(94),
            "Drukarka Novitus zwróciła błąd 94 ($h). Sprzedaż zablokowana: założona zwora \
             serwisowa.",
        ),
        // The same number means a parameter error for `$h`, not a date range.
        (receipt_refusal(4), "Drukarka Novitus zwróciła błąd 4 ($h)."),
    ] {
        assert!(matches!(error, Error::Device { .. }), "{error:?}");
        assert_eq!(error.to_string(), text);
    }
}

#[test]
fn preflight_refusals_are_rechecked_before_receipts_and_reports() {
    for problem in 0..11 {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let message = match problem {
            0 => {
                sim.set_status(0x74, 0x6F, "@s0;0;0;00000000;");
                "Drukarka jest zajęta, ma otwarty dokument lub wymaga obsługi"
            },
            1 => {
                sim.set_status(0x76, 0x6D, "@s0;0;0;00000000;");
                "Drukarka jest zajęta, ma otwarty dokument lub wymaga obsługi"
            },
            2 => {
                sim.set_status(0x74, 0x6D, "@s0;0;0;00000001;");
                "Drukarka jest zajęta, ma otwarty dokument lub wymaga obsługi"
            },
            3 => {
                sim.set_fiscal_mode(2);
                "Drukarka jest w trybie tylko do odczytu"
            },
            4 => {
                sim.set_header("");
                "Brak zaprogramowanego nagłówka"
            },
            5 => {
                sim.set_config_option(91, "0");
                "Ustaw wejście i wyjście PC na Windows-1250"
            },
            6 => {
                sim.set_config_option(92, "0");
                "Ustaw wejście i wyjście PC na Windows-1250"
            },
            7 | 10 => {
                sim.set_dialect(if problem == 7 { "0" } else { "2" });
                "Wymagany jest natywny protokół NOVITUS"
            },
            8 => {
                sim.set_endpoint("PC2");
                "Nieprawidłowy numer unikatowy drukarki"
            },
            _ => {
                sim.set_vat(["99.99"; 7]);
                "Brak aktywnych stawek VAT"
            },
        };
        assert_eq!(driver.probe().unwrap().description(), message);
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::NotReady(text)) if text == message
        ));
        assert!(matches!(
            driver.print_report(Report::Daily, "ABC1234567890", &mut log),
            Err(Error::NotReady(text)) if text == message
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        assert_eq!((sim.receipts(), sim.reports()), (0, 0));
    }
}

#[test]
fn non_cp1250_output_reports_the_code_page_instead_of_a_corrupt_header() {
    // CP-852 "ł" is 0x88, a byte Windows-1250 leaves unassigned.
    let header = b"1#UWarsztat Kowalski, ul. G\x88ogowska 1";
    for discovery in [false, true] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_config_option(92, "3");
        let mut reply = frame::encode(header);
        if !discovery {
            reply.extend_from_slice(&frame::query(b"0#Z^u"));
        }
        sim.fault("^u", Fault::Reply(reply));
        let device = if discovery {
            identify(sim.link().as_ref(), &connection(&sim))
                .unwrap()
                .unwrap()
        } else {
            printer(&sim).probe().unwrap()
        };
        assert_eq!(
            device.description(),
            "Ustaw wejście i wyjście PC na Windows-1250",
            "discovery: {discovery}"
        );
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn dle_or_dle2_problem_is_reported_without_waiting_for_an_unanswered_enq() {
    // Paper out, mechanism error, off-line; then a healthy DLE with DLE2 activity flags.
    for (dle, extended) in [
        (0x76, "@s0;0;0;00000000;"),
        (0x75, "@s0;0;0;00000000;"),
        (0x70, "@s0;0;0;00000000;"),
        (0x74, "@s0;0;0;00000080;"),
        (0x74, "@s0;0;0;00008000;"),
        (0x74, "@s0;0;0;00800000;"),
        (0x74, "@s1530;0;0;00000000;"),
        (0x74, "@s0;0;0;01000000;"),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_status(dle, 0x6D, extended);
        let device = printer(&sim).probe().unwrap();
        assert_eq!(
            device.description(),
            "Drukarka jest zajęta, ma otwarty dokument lub wymaga obsługi",
            "DLE {dle:#04X}, {extended}"
        );
        let found = identify(sim.link().as_ref(), &connection(&sim))
            .unwrap()
            .unwrap();
        assert_eq!(
            found.description(),
            device.description(),
            "DLE {dle:#04X}, {extended}"
        );
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        assert!(!sim.frames().iter().any(|raw| raw == &[super::ENQ]));
    }
}

#[test]
fn journal_failure_prevents_send_and_is_uncertain_only_after_begin() {
    for fail_at in 0..RECEIPT_FRAMES.len() {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut log = Log {
            fail_at: Some(fail_at),
            ..Log::default()
        };
        let error = driver
            .print_receipt(&receipt(vec![]), &expected, &mut log)
            .unwrap_err();
        if fail_at == 0 {
            assert!(matches!(error, Error::Journal(_)));
        } else {
            assert!(matches!(error, Error::Uncertain(_)));
        }
        assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES[..fail_at]);
        assert_eq!(log.commands, RECEIPT_COMMANDS[..fail_at]);
        assert_eq!(sim.receipts(), 0);
    }
}

#[test]
fn operator_wait_resumes_without_resending() {
    struct Operator<'a> {
        sim: &'a Simulator,
        events: Vec<Option<String>>,
    }
    impl Journal for Operator<'_> {
        fn dispatching(&mut self, _: &'static str) -> io::Result<()> {
            Ok(())
        }

        fn waiting(&mut self, problem: Option<&str>) {
            self.events.push(problem.map(str::to_owned));
            if problem.is_some() {
                self.sim.resume();
            }
        }
    }
    let sim = Simulator::start();
    supported(&sim);
    sim.fault("#r", Fault::Wait);
    let mut journal = Operator {
        sim: &sim,
        events: vec![],
    };
    printer(&sim)
        .print_report(Report::Daily, "ABC1234567890", &mut journal)
        .unwrap();
    assert_eq!(
        journal.events,
        [
            Some("Brak papieru lub niskie napięcie akumulatora".to_owned()),
            None
        ]
    );
    assert_eq!(fiscal_bodies(&sim), ["1;26;10;24#r"]);
    assert_eq!(sim.reports(), 1);

    // Paper runs out on the first item: the receipt must finish without resending it.
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.fault("$l", Fault::Wait);
    let mut journal = Operator {
        sim: &sim,
        events: vec![],
    };
    let printed = driver
        .print_receipt(&receipt(vec![]), &expected, &mut journal)
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(
        journal.events,
        [
            Some("Brak papieru lub niskie napięcie akumulatora".to_owned()),
            None
        ]
    );
    assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES);
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn monthly_and_periodic_reports_use_calendar_ranges() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    for report in [
        Report::Monthly {
            year: 2024,
            month: 2,
        },
        Report::Periodic {
            from: "2024-01-02".parse().unwrap(),
            to: "2024-03-04".parse().unwrap(),
            summary: true,
        },
        Report::Periodic {
            from: "2024-01-02".parse().unwrap(),
            to: "2024-03-04".parse().unwrap(),
            summary: false,
        },
    ] {
        driver
            .print_report(report, "ABC1234567890", &mut Log::default())
            .unwrap();
    }
    assert_eq!(
        fiscal_bodies(&sim),
        [
            "24;2;1;24;2;29;6#o",
            "24;1;2;24;3;4;1#o",
            "24;1;2;24;3;4;0#o"
        ]
    );
}

#[test]
fn periodic_report_past_printer_date_or_inverted_never_dispatches() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let periodic = |from: &str, to: &str| Report::Periodic {
        from: from.parse().unwrap(),
        to: to.parse().unwrap(),
        summary: false,
    };
    // The simulated printer date is 2026-10-24.
    for report in [
        periodic("2026-10-01", "2026-10-25"),
        periodic("2026-10-20", "2026-10-19"),
    ] {
        let mut log = Log::default();
        assert!(matches!(
            driver.print_report(report, "ABC1234567890", &mut log),
            Err(Error::Invalid(message)) if message == "Nieprawidłowy zakres dat raportu."
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
    }
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    driver
        .print_report(
            periodic("2026-10-24", "2026-10-24"),
            "ABC1234567890",
            &mut Log::default(),
        )
        .unwrap();
    assert_eq!(fiscal_bodies(&sim), ["26;10;24;26;10;24;0#o"]);
}

#[test]
fn monthly_report_of_the_printer_month_never_dispatches() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let monthly = |month| Report::Monthly { year: 2026, month };
    // Printer dates 2026-10-24 and 2026-10-31: October has not ended on the printer.
    for clock in ["26;10;24;12;0;0", "26;10;31;23;59;59"] {
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
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    driver
        .print_report(monthly(9), "ABC1234567890", &mut Log::default())
        .unwrap();
    assert_eq!(fiscal_bodies(&sim), ["26;9;1;26;9;30;6#o"]);
}

#[cfg(not(workshop_acceptance))]
#[test]
fn brand_across_padded_header_lines_needs_no_footer() {
    // `^u` pads header lines with spaces instead of sending line breaks.
    for (header, printed) in [
        ("     AUTO SERWIS                   KOWALSKI      ", true),
        ("AUTOSERWIS KOWALSKI", false),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_header(header);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let base = receipt(vec![]);
        let receipt = Receipt::new(
            base.lines().to_vec(),
            vec![],
            base.payments().to_vec(),
            None,
            "Auto Serwis Kowalski",
            &expected.capabilities,
        )
        .unwrap();
        let result = driver.print_receipt(&receipt, &expected, &mut Log::default());
        if printed {
            assert!(result.is_ok(), "{header:?}: {result:?}");
            assert_eq!(fiscal_bodies(&sim).first().map(String::as_str), Some("0$h"));
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(
                matches!(&result, Err(Error::Invalid(text)) if text == FOOTER_REFUSAL),
                "{header:?}: {result:?}"
            );
            assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        }
    }
}

#[test]
fn receipt_stage_faults_preserve_the_exact_prefix_and_never_retry() {
    for (index, body) in RECEIPT_FRAMES.iter().enumerate() {
        for fault in [
            Fault::LoseReply,
            Fault::CorruptReply,
            Fault::Disconnect,
            Fault::Refuse(40),
            Fault::Reply(frame::query(b"0#Z#r")),
        ] {
            let sim = Simulator::start();
            supported(&sim);
            let driver = printer(&sim);
            let expected = driver.probe().unwrap();
            let refused = matches!(fault, Fault::Refuse(_));
            sim.fault(body, fault);
            let mut log = Log::default();
            let error = driver
                .print_receipt(&receipt(vec![]), &expected, &mut log)
                .unwrap_err();
            if index == 0 && refused {
                assert!(
                    matches!(error, Error::Device { code: 40, command, .. } if command == "$h")
                );
            } else {
                assert!(matches!(error, Error::Uncertain(_)), "{index}: {error:?}");
            }
            assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES[..=index]);
            assert_eq!(log.commands, RECEIPT_COMMANDS[..=index]);
            assert_eq!(sim.receipts(), u32::from(index == 12 && !refused));
            assert_eq!(sim.reports(), 0);
            assert_eq!(
                driver.probe().unwrap().ready(),
                (index == 0 && refused) || (index == 12 && !refused),
                "open receipt state after stage {index}"
            );
        }
    }
}

#[test]
fn post_success_counter_failure_does_not_reprint() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.fault("?l", Fault::LoseReply);
    let printed = driver
        .print_receipt(&receipt(vec![]), &expected, &mut Log::default())
        .unwrap();
    assert_eq!(printed.receipt_number, None);
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn stale_vat_wrong_device_and_inactive_slot_never_begin() {
    for problem in 0..4 {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let mut expected = driver.probe().unwrap();
        match problem {
            0 => expected.unique_number = "OTHER12345678".to_owned(),
            1 => sim.set_vat(["22", "8", "5", "0", "98.99", "99.99", "99.99"]),
            // Slot G may hold any rate, so the device stays ready; only the guard refuses.
            2 => sim.set_vat(["23", "8", "5", "0", "98.99", "99.99", "8.00"]),
            _ => {
                sim.set_vat(["23", "8", "5", "0", "99.99", "99.99", "99.99"]);
                expected = driver.probe().unwrap();
            },
        }
        let result = driver.print_receipt(&receipt(vec![]), &expected, &mut Log::default());
        let message = match problem {
            0 => {
                "Podłączona drukarka ABC1234567890 nie jest drukarką OTHER12345678, \
                 której dotyczy operacja."
            },
            1 => "Nieprawidłowe przypisanie liter VAT A–E",
            2 => "Drukarka lub stawki VAT zmieniły się. Odśwież połączenie.",
            _ => "Wybrana stawka VAT jest nieaktywna.",
        };
        if problem == 1 {
            assert!(matches!(result, Err(Error::NotReady(text)) if text == message));
        } else {
            assert!(matches!(result, Err(Error::Invalid(text)) if text == message));
        }
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn invalid_printer_clock_blocks_every_operation() {
    for clock in [
        "26;2;30;12;0;0",
        "50;1;1;12;0;0",
        "26;+1;24;12;0;0",
        "26;010;24;12;0;0",
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.set_clock(clock);
        let mut log = Log::default();
        assert!(matches!(driver.probe(), Err(Error::Protocol(_))));
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(
            driver.print_report(Report::Daily, "ABC1234567890", &mut log),
            Err(Error::Protocol(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn dst_fold_or_gap_clock_blocks_only_receipt_timestamps() {
    for clock in ["26;10;25;2;30;0", "26;3;29;2;30;0"] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_clock(clock);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(expected.ready(), "{clock}: {}", expected.description());
        let _resolvable = driver.verify("ABC1234567890").unwrap();
        driver
            .print_report(
                Report::Periodic {
                    from: "2026-01-01".parse().unwrap(),
                    to: "2026-01-31".parse().unwrap(),
                    summary: false,
                },
                "ABC1234567890",
                &mut Log::default(),
            )
            .unwrap();
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::Protocol(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), ["26;1;1;26;1;31;0#o"]);
    }
}

#[test]
fn journal_precedes_every_fiscal_send() {
    struct Ordered<'a> {
        sim: &'a Simulator,
        log: Vec<&'static str>,
    }
    impl Journal for Ordered<'_> {
        fn dispatching(&mut self, command: &'static str) -> io::Result<()> {
            if fiscal_bodies(self.sim).len() != self.log.len() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            self.log.push(command);
            Ok(())
        }

        fn waiting(&mut self, _: Option<&str>) {}
    }
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let mut journal = Ordered {
        sim: &sim,
        log: vec![],
    };
    let _printed = driver
        .print_receipt(&receipt(vec![]), &expected, &mut journal)
        .unwrap();
    assert_eq!(journal.log, RECEIPT_COMMANDS);
    assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES);
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn waiting_cap_and_unexplained_silence_never_resend_or_guess_dle2_order() {
    for (fault, waits) in [
        (
            Fault::Wait,
            vec![
                Some("Brak papieru lub niskie napięcie akumulatora".to_owned()),
                None,
            ],
        ),
        (Fault::LoseReply, vec![]),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        sim.fault("#r", fault);
        let mut journal = Log::default();
        assert!(matches!(
            printer(&sim).print_report(Report::Daily, "ABC1234567890", &mut journal),
            Err(Error::Uncertain(_))
        ));
        assert_eq!(fiscal_bodies(&sim), ["1;26;10;24#r"]);
        assert_eq!(sim.reports(), 1);
        assert_eq!(journal.waits, waits);
        assert_eq!(journal.commands, ["#r"]);
        let frames = sim.frames();
        let dispatched = frames
            .iter()
            .position(|raw| frame::decode(raw).is_ok_and(|body| body == b"1;26;10;24#r"))
            .unwrap();
        assert!(
            !frames[dispatched + 1..]
                .iter()
                .any(|raw| raw == &[super::ENQ])
        );
    }
}

#[test]
fn receipt_is_revalidated_against_actual_model_limits_before_begin() {
    for (model, firmware, name, quantity, nip) in [
        (
            "NOVITUS DEON ONLINE",
            "1.40",
            "123456789012345678901234567890123456789012345",
            "1",
            "1234567890",
        ),
        (
            "NOVITUS HD II ONLINE",
            "1.40",
            "Naprawa",
            "999999999.12",
            "1234567890",
        ),
        ("NOVITUS HD II ONLINE", "1.40", "Naprawa", "1", "123/456"),
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, firmware);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut caps = expected.capabilities.clone();
        caps.max_item_name = 60;
        let line = Line::new(
            name,
            Quantity::parse(quantity).unwrap(),
            Grosze::new(1).unwrap(),
            VatSlot::new(0).unwrap(),
            &caps,
        )
        .unwrap();
        let job = Receipt::new(
            vec![line.clone()],
            vec![],
            vec![Payment {
                form: PaymentForm::Cash,
                amount: line.total(),
            }],
            Some(nip.to_owned()),
            "Warsztat",
            &caps,
        )
        .unwrap();
        assert!(matches!(
            driver.print_receipt(&job, &expected, &mut Log::default()),
            Err(Error::Invalid(_))
        ));
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn polish_names_print_when_device_reports_full_cp1250() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(
        expected
            .capabilities
            .allows("Przegląd Łódź żółć € ¤ ¨ ˛ ´ ¸")
    );
    assert!(!expected.capabilities.allows("\r\n\u{7f}\u{1b}"));
    let identified = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    assert_eq!(identified.capabilities, expected.capabilities);
    let line = Line::new(
        "Przegląd",
        Quantity::parse("1").unwrap(),
        Grosze::new(2000).unwrap(),
        VatSlot::new(0).unwrap(),
        &expected.capabilities,
    )
    .unwrap();
    let job = Receipt::new(
        vec![line],
        vec![],
        vec![Payment {
            form: PaymentForm::Card,
            amount: Grosze::new(2000).unwrap(),
        }],
        None,
        "Warsztat",
        &expected.capabilities,
    )
    .unwrap();
    let printed = driver
        .print_receipt(&job, &expected, &mut Log::default())
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    insta::assert_debug_snapshot!("polish_receipt_frames", fiscal_bodies(&sim));
    assert!(
        sim.frames()
            .iter()
            .any(|raw| raw.windows(8).any(|part| part == b"Przegl\xB9d")),
        "name is unchanged CP1250"
    );
    assert_eq!(
        bodies(&sim)
            .iter()
            .filter(|body| body.as_str() == "52$i")
            .count(),
        3
    );
}

#[test]
fn ascii_only_without_cp1250_capability_bit() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_full_cp1250(false);
    let device = printer(&sim).probe().unwrap();
    assert_eq!(
        device.capabilities.allowed_characters,
        (' '..='~').collect::<String>()
    );
    assert!(device.capabilities.allows("Przeglad"));
    assert!(!device.capabilities.allows("Przegląd"));
    let found = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    assert_eq!(found.capabilities, device.capabilities);
    assert_eq!(sim.receipts(), 0);
}

#[test]
fn unreadable_cp1250_capability_keeps_ascii_and_never_uses_a_blocked_session() {
    let mut malformed = frame::encode(
        b"52$I00000200\r00000000\rnot-hex!\r00000000\r00000000\r00000000\r00000000\r00000000\r",
    );
    malformed.extend_from_slice(&frame::query(b"0#Z$i"));
    for fault in [
        Fault::Refuse(82),
        Fault::Reply(malformed),
        Fault::LoseReply,
        Fault::CorruptReply,
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let transport_lost = matches!(fault, Fault::LoseReply | Fault::CorruptReply);
        sim.fault("52$i", fault);
        let device = printer(&sim).probe().unwrap();
        assert_eq!(
            device.capabilities.allowed_characters,
            (' '..='~').collect::<String>()
        );
        assert_eq!(device.ready(), !transport_lost);
        assert_eq!(sim.receipts(), 0);
    }
}

#[test]
fn printing_session_checks_cp1250_capability_again_before_begin() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    assert!(expected.capabilities.allows("Przegląd"));
    let line = Line::new(
        "Przegląd",
        Quantity::parse("1").unwrap(),
        Grosze::new(2000).unwrap(),
        VatSlot::new(0).unwrap(),
        &expected.capabilities,
    )
    .unwrap();
    let job = Receipt::new(
        vec![line],
        vec![],
        vec![Payment {
            form: PaymentForm::Card,
            amount: Grosze::new(2000).unwrap(),
        }],
        None,
        "Warsztat",
        &expected.capabilities,
    )
    .unwrap();
    sim.set_full_cp1250(false);
    assert!(matches!(
        driver.print_receipt(&job, &expected, &mut Log::default()),
        Err(Error::Invalid(_))
    ));
    assert_eq!(sim.receipts(), 0);
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
}

#[test]
fn discovery_identifies_printer_in_power_on_error_mode() {
    let sim = Simulator::start();
    supported(&sim);
    let device = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    assert!(device.ready(), "{}", device.description());
    assert_eq!(bodies(&sim).first().map(String::as_str), Some("30#v"));
    assert!(!bodies(&sim).iter().any(|body| body == "3#e"));
}

#[test]
fn discovery_tolerates_printer_already_in_mode_three() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_error_mode(3);
    sim.set_fragment_size(1);
    let device = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    assert!(device.ready(), "{}", device.description());
    assert!(!bodies(&sim).iter().any(|body| body == "3#e"));
}

#[test]
fn printing_session_selects_mode_three_on_a_power_on_printer() {
    let sim = Simulator::start();
    supported(&sim);
    let device = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    let printed = printer(&sim)
        .print_receipt(&receipt(vec![]), &device, &mut Log::default())
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(sim.receipts(), 1);
    assert!(bodies(&sim).iter().any(|body| body == "3#e"));
}

#[test]
fn persisting_operator_wait_is_announced_once() {
    let sim = Simulator::start();
    supported(&sim);
    sim.fault("#r", Fault::WaitPolls(3));
    let mut journal = Log::default();
    printer(&sim)
        .print_report(Report::Daily, "ABC1234567890", &mut journal)
        .unwrap();
    assert_eq!(
        journal.waits,
        [
            Some("Brak papieru lub niskie napięcie akumulatora".to_owned()),
            None
        ]
    );
    assert_eq!(fiscal_bodies(&sim), ["1;26;10;24#r"]);
    assert_eq!(sim.reports(), 1);
}

#[test]
fn discovery_consumes_its_late_acks_before_closing() {
    // PC1 ends after `52$i`; PC2 returns early after `50$i`.
    for (endpoint, query) in [("PC1", "52$i"), ("PC2", "50$i")] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_error_mode(3);
        sim.set_endpoint(endpoint);
        sim.fault(query, Fault::DeferAck);
        let found = identify(sim.link().as_ref(), &connection(&sim)).unwrap();
        assert_eq!(found.is_some(), endpoint == "PC1");
        let device = printer(&sim).probe().unwrap();
        assert_eq!(
            device.ready(),
            endpoint == "PC1",
            "{}",
            device.description()
        );
        if let Some(found) = found {
            assert_eq!(device.capabilities, found.capabilities);
        }
        assert_eq!(sim.receipts(), 0);
    }
}

#[test]
fn discovery_refusal_in_power_on_mode_gets_no_reply_and_is_not_ready() {
    let sim = Simulator::start();
    supported(&sim);
    sim.fault("52$i", Fault::Refuse(82));
    let device = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    assert!(!device.ready());
    assert_eq!(
        device.description(),
        "Nie można bezpiecznie odczytać możliwości znaków drukarki"
    );
    assert_eq!(
        device.capabilities.allowed_characters,
        (' '..='~').collect::<String>()
    );
    assert!(!bodies(&sim).iter().any(|body| body == "3#e"));
}

#[test]
fn reports_require_matching_identity_and_a_journal_before_dispatch() {
    for (report, body, command) in [
        (Report::Daily, "1;26;10;24#r", "#r"),
        (
            Report::Monthly {
                year: 2024,
                month: 2,
            },
            "24;2;1;24;2;29;6#o",
            "#o",
        ),
        (
            Report::Periodic {
                from: "2024-01-02".parse().unwrap(),
                to: "2024-03-04".parse().unwrap(),
                summary: false,
            },
            "24;1;2;24;3;4;0#o",
            "#o",
        ),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let mut log = Log::default();
        assert!(matches!(
            driver.print_report(report, "OTHER12345678", &mut log),
            Err(Error::Invalid(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        log.fail_at = Some(0);
        assert!(matches!(
            driver.print_report(report, "ABC1234567890", &mut log),
            Err(Error::Journal(error)) if error.kind() == io::ErrorKind::StorageFull
        ));
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        assert_eq!(sim.reports(), 0);
        log.fail_at = None;
        driver
            .print_report(report, "ABC1234567890", &mut log)
            .unwrap();
        assert_eq!(fiscal_bodies(&sim), [body]);
        assert_eq!(log.commands, [command]);
        assert_eq!(sim.reports(), 1);
    }
}

#[test]
fn executed_report_with_lost_corrupt_disconnected_or_mismatched_reply_is_uncertain() {
    for (report, body, command) in [
        (Report::Daily, "1;26;10;24#r", "#r"),
        (
            Report::Monthly {
                year: 2024,
                month: 2,
            },
            "24;2;1;24;2;29;6#o",
            "#o",
        ),
        (
            Report::Periodic {
                from: "2024-01-02".parse().unwrap(),
                to: "2024-03-04".parse().unwrap(),
                summary: true,
            },
            "24;1;2;24;3;4;1#o",
            "#o",
        ),
    ] {
        for fault in [
            Fault::LoseReply,
            Fault::CorruptReply,
            Fault::Disconnect,
            Fault::Reply(frame::query(b"36#Z$h")),
            Fault::Reply(frame::query(b"+36#Z#r")),
        ] {
            let sim = Simulator::start();
            supported(&sim);
            sim.fault(body, fault);
            let mut log = Log::default();
            assert!(matches!(
                printer(&sim).print_report(report, "ABC1234567890", &mut log),
                Err(Error::Uncertain(_))
            ));
            assert_eq!(fiscal_bodies(&sim), [body]);
            assert_eq!(log.commands, [command]);
            assert_eq!((sim.receipts(), sim.reports()), (0, 1));
        }
    }
}

#[test]
fn report_year_outside_wire_epoch_is_refused_without_dispatch() {
    for report in [
        Report::Monthly {
            year: 1999,
            month: 12,
        },
        Report::Periodic {
            from: "1999-12-31".parse().unwrap(),
            to: "2000-01-01".parse().unwrap(),
            summary: false,
        },
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let mut log = Log::default();
        assert!(matches!(
            printer(&sim).print_report(report, "ABC1234567890", &mut log),
            Err(Error::Invalid(message))
                if message == "Rok raportu musi należeć do zakresu 2000–2099."
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        assert_eq!(sim.reports(), 0);
    }
}

#[test]
fn unknown_vat_values_never_reach_fiscal_dispatch() {
    for value in ["101.01", "102", "-1", "NaN", "23.001"] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.set_vat(["23", "8", "5", "0", value, "99.99", "99.99"]);
        let mut log = Log::default();
        assert!(matches!(driver.probe(), Err(Error::Protocol(_))), "{value}");
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::Protocol(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn line_and_payment_limits_are_revalidated_at_the_driver_boundary() {
    for (lines, payments, accepted, message) in [
        (255, 16, true, ""),
        (
            256,
            1,
            false,
            "Paragon przekracza limity tekstu lub liczby pozycji Novitus.",
        ),
        (
            1,
            17,
            false,
            "Novitus obsługuje najwyżej 16 form płatności.",
        ),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut caps = expected.capabilities.clone();
        caps.max_lines = 256;
        let line = Line::new(
            "Limit",
            Quantity::parse("1").unwrap(),
            Grosze::new(payments).unwrap(),
            VatSlot::new(0).unwrap(),
            &caps,
        )
        .unwrap();
        let job = Receipt::new(
            vec![line; lines],
            vec![],
            vec![
                Payment {
                    form: PaymentForm::Cash,
                    amount: Grosze::new(u64::try_from(lines).unwrap()).unwrap()
                };
                usize::try_from(payments).unwrap()
            ],
            None,
            "Warsztat",
            &caps,
        )
        .unwrap();
        let mut log = Log::default();
        let result = driver.print_receipt(&job, &expected, &mut log);
        if accepted {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            let mut frames = vec!["0$h".to_owned()];
            frames.extend((1..=255).map(|index| format!("{index}$lLimit\r1\rA/0.16/0.16/")));
            frames.extend(vec!["1;0$b2.55/\r".to_owned(); 16]);
            frames.extend(
                [
                    "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r40.8/0/0/40.8/0/",
                    "12$z",
                    "28;0$z",
                ]
                .map(str::to_owned),
            );
            assert_eq!(fiscal_bodies(&sim), frames);
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(matches!(result, Err(Error::Invalid(text)) if text == message));
            assert_eq!(log.commands, Vec::<&str>::new());
            assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
            assert_eq!(sim.receipts(), 0);
        }
    }
}

#[test]
fn operator_wait_reason_changes_and_resume_do_not_repeat_the_report() {
    struct Operator<'a> {
        sim: &'a Simulator,
        events: Vec<Option<String>>,
    }
    impl Journal for Operator<'_> {
        fn dispatching(&mut self, _: &'static str) -> io::Result<()> {
            Ok(())
        }

        fn waiting(&mut self, problem: Option<&str>) {
            self.events.push(problem.map(str::to_owned));
            match problem {
                Some("Brak papieru lub niskie napięcie akumulatora") => {
                    self.sim.set_status(0x75, 0x6D, "@s0;0;0;00000000;");
                },
                Some("Błąd mechanizmu drukarki") => self.sim.resume(),
                _ => {},
            }
        }
    }
    let sim = Simulator::start();
    supported(&sim);
    sim.fault("#r", Fault::Wait);
    let mut journal = Operator {
        sim: &sim,
        events: vec![],
    };
    printer(&sim)
        .print_report(Report::Daily, "ABC1234567890", &mut journal)
        .unwrap();
    assert_eq!(
        journal.events,
        [
            Some("Brak papieru lub niskie napięcie akumulatora".to_owned()),
            Some("Błąd mechanizmu drukarki".to_owned()),
            None,
        ]
    );
    assert_eq!(fiscal_bodies(&sim), ["1;26;10;24#r"]);
    assert_eq!(sim.reports(), 1);
}

#[test]
fn corrupt_wait_status_after_execution_is_uncertain_without_a_resend() {
    struct CorruptStatus<'a>(&'a Simulator);
    impl Journal for CorruptStatus<'_> {
        fn dispatching(&mut self, _: &'static str) -> io::Result<()> {
            self.0.set_status(0x74, 0x6D, "@s0;0;0;not-flags;");
            Ok(())
        }

        fn waiting(&mut self, _: Option<&str>) {}
    }
    let sim = Simulator::start();
    supported(&sim);
    sim.fault("#r", Fault::Wait);
    assert!(matches!(
        printer(&sim).print_report(Report::Daily, "ABC1234567890", &mut CorruptStatus(&sim)),
        Err(Error::Uncertain(error)) if matches!(*error, Error::Protocol(_))
    ));
    assert_eq!(fiscal_bodies(&sim), ["1;26;10;24#r"]);
    assert_eq!(sim.reports(), 1);
}

#[test]
fn quantity_and_rounded_grosze_are_unchanged_on_the_wire() {
    // NOVITUS v1.15 §3.4.2: ten quantity digits; compare rounded quantity × price.
    for (quantity, grosze, amount) in [
        ("0.5", 1, "0.01"),
        ("1.5", 2, "0.02"),
        ("1.005", 1, "0.01"),
        ("99999999.99", 100_000_000, "1000000"),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let caps = &expected.capabilities;
        let line = Line::new(
            "Ilość",
            Quantity::parse(quantity).unwrap(),
            Grosze::new(1).unwrap(),
            VatSlot::new(0).unwrap(),
            caps,
        )
        .unwrap();
        let job = Receipt::new(
            vec![line],
            vec![],
            vec![Payment {
                form: PaymentForm::Cash,
                amount: Grosze::new(grosze).unwrap(),
            }],
            None,
            "Warsztat",
            caps,
        )
        .unwrap();
        let printed = driver
            .print_receipt(&job, &expected, &mut Log::default())
            .unwrap();
        assert_eq!(printed.receipt_number.as_deref(), Some("1"));
        assert_eq!(
            fiscal_bodies(&sim),
            [
                "0$h".to_owned(),
                format!("1$lIlość\r{quantity}\rA/0.01/{amount}/"),
                format!("1;0$b{amount}/\r"),
                format!("0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r{amount}/0/0/{amount}/0/"),
                "12$z".to_owned(),
                "28;0$z".to_owned(),
            ]
        );
        assert_eq!(sim.receipts(), 1);
    }
}

fn status_reply(status: &str, unique: &str, suffix: &str) -> Vec<u8> {
    frame::encode(
        format!("2#X{status}/23/8/5/0/98.99/99.99/99.99/0/0/0/0/0/0/0/0/0/{unique}{suffix}")
            .as_bytes(),
    )
}

#[test]
fn each_fiscal_status_source_is_checked_independently_in_the_execution_session() {
    for (mode, enq, status, problem) in [
        (
            0,
            0x6D,
            "0;1;0;1;1;0;26;10;24",
            "Drukarka nie jest w czynnym trybie fiskalnym",
        ),
        (
            1,
            0x65,
            "0;1;0;1;1;0;26;10;24",
            "Drukarka jest zajęta, ma otwarty dokument lub wymaga obsługi",
        ),
        (
            1,
            0x6D,
            "0;0;0;1;1;0;26;10;24",
            "Status fiskalny wskazuje otwartą transakcję lub błąd",
        ),
        (
            1,
            0x6D,
            "1;1;0;1;1;0;26;10;24",
            "Status fiskalny wskazuje otwartą transakcję lub błąd",
        ),
        (
            1,
            0x6D,
            "0;1;1;1;1;0;26;10;24",
            "Status fiskalny wskazuje otwartą transakcję lub błąd",
        ),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.set_fiscal_mode(mode);
        sim.set_status(0x74, enq, "@s0;0;0;00000000;");
        sim.fault(
            "23#s",
            Fault::Reply(status_reply(status, "ABC1234567890", "")),
        );
        let mut log = Log::default();
        let result = driver.print_receipt(&receipt(vec![]), &expected, &mut log);
        if cfg!(workshop_acceptance) && (mode == 0 || enq == 0x65 || status.starts_with("0;0;")) {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES);
            assert_eq!(log.commands, RECEIPT_COMMANDS);
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(matches!(result, Err(Error::NotReady(message)) if message == problem));
            assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
            assert_eq!(log.commands, Vec::<&str>::new());
            assert_eq!(sim.receipts(), 0);
        }
    }
}

#[test]
fn malformed_status_and_unique_number_are_protocol_errors_before_dispatch() {
    for (status, unique, suffix) in [
        ("0;1;0;1;1;0;26;10", "ABC1234567890", ""),
        ("0;1;0;1;1;0;26;10;", "ABC1234567890", ""),
        ("0;1;0;1;1;0;26;10;x", "ABC1234567890", ""),
        ("0;1;0;1;1;0;26;10;24", "", ""),
        ("0;1;0;1;1;0;26;10;24", "ABC123456789!", ""),
        ("0;1;0;1;1;0;26;10;24", "ABC1234567890", "/extra"),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault("23#s", Fault::Reply(status_reply(status, unique, suffix)));
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::Protocol(message))
                if message == "Uszkodzona ramka Novitus (suma kontrolna lub format)."
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        assert_eq!(sim.receipts(), 0);
    }
}

#[test]
fn receipt_number_readback_never_returns_empty_or_non_decimal_numbers() {
    for number in ["", "ABC", "1A", "+1"] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut reply = frame::encode(format!("0?l2/0/{number}/").as_bytes());
        reply.extend(frame::query(b"0#Z?l"));
        sim.fault("?l", Fault::Reply(reply));
        let printed = driver
            .print_receipt(&receipt(vec![]), &expected, &mut Log::default())
            .unwrap();
        assert_eq!(printed.receipt_number, None, "{number:?}");
        assert_eq!(fiscal_bodies(&sim), RECEIPT_FRAMES);
        assert_eq!(sim.receipts(), 1);
    }
}

#[test]
fn exact_text_and_nip_limits_accept_the_boundary_and_reject_excess_before_begin() {
    for (name_len, brand_len, nip, accepted, message) in [
        (60, 40, "ABCDEFGHIJKLMNOP", true, ""),
        (
            61,
            40,
            "ABCDEFGHIJKLMNOP",
            false,
            "Nazwa pozycji przekracza możliwości Novitus.",
        ),
        (
            60,
            41,
            "ABCDEFGHIJKLMNOP",
            false,
            "Paragon przekracza limity tekstu lub liczby pozycji Novitus.",
        ),
        (
            60,
            40,
            "ABCDEFGHIJKLMNOPQ",
            false,
            "Paragon przekracza limity tekstu lub liczby pozycji Novitus.",
        ),
    ] {
        let sim = Simulator::start();
        supported(&sim);
        let brand = "B".repeat(brand_len);
        sim.set_header(&brand);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        let mut caps = expected.capabilities.clone();
        caps.max_item_name = 61;
        caps.max_company_name = 41;
        caps.max_buyer_nip = 17;
        let name = "A".repeat(name_len);
        let line = Line::new(
            &name,
            Quantity::parse("1").unwrap(),
            Grosze::new(100).unwrap(),
            VatSlot::new(0).unwrap(),
            &caps,
        )
        .unwrap();
        let job = Receipt::new(
            vec![line],
            vec![],
            vec![Payment {
                form: PaymentForm::Cash,
                amount: Grosze::new(100).unwrap(),
            }],
            Some(nip.to_owned()),
            &brand,
            &caps,
        )
        .unwrap();
        let mut log = Log::default();
        let result = driver.print_receipt(&job, &expected, &mut log);
        if accepted {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            assert_eq!(
                fiscal_bodies(&sim),
                [
                    "0$h".to_owned(),
                    "100;1$zABCDEFGHIJKLMNOP\r".to_owned(),
                    format!("1$l{name}\r1\rA/1/1/"),
                    "1;0$b1/\r".to_owned(),
                    "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r1/0/0/1/0/".to_owned(),
                    "12$z".to_owned(),
                    "28;0$z".to_owned(),
                ]
            );
            assert_eq!(sim.receipts(), 1);
        } else {
            assert!(matches!(result, Err(Error::Invalid(text)) if text == message));
            assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
            assert_eq!(log.commands, Vec::<&str>::new());
            assert_eq!(sim.receipts(), 0);
        }
    }
}

#[test]
fn company_repertoire_is_revalidated_even_when_the_header_already_contains_it() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_header("Łódź");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let base = receipt(vec![]);
    let job = Receipt::new(
        base.lines().to_vec(),
        vec![],
        base.payments().to_vec(),
        None,
        "Łódź",
        &expected.capabilities,
    )
    .unwrap();
    sim.set_full_cp1250(false);
    let mut log = Log::default();
    assert!(matches!(
        driver.print_receipt(&job, &expected, &mut log),
        Err(Error::Invalid(message))
            if message == "Paragon przekracza limity tekstu lub liczby pozycji Novitus."
    ));
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    assert_eq!(log.commands, Vec::<&str>::new());
    assert_eq!(sim.receipts(), 0);
}

#[test]
fn discovery_ack_before_capability_data_is_already_settled() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_error_mode(3);
    let mut reply = frame::query(b"0#Z$i");
    reply.extend(frame::encode(
        b"52$I00000200\r00000000\r00000000\r00000000\r00000000\r00000000\r00000000\r00000000\r",
    ));
    sim.fault("52$i", Fault::Reply(reply));
    let found = identify(sim.link().as_ref(), &connection(&sim))
        .unwrap()
        .unwrap();
    assert!(found.ready(), "{}", found.description());
    assert_eq!(
        bodies(&sim),
        [
            "30#v",
            "50$i",
            "0;1^c5/5/",
            "0;1^c91/92/",
            "11$i",
            "^u",
            "23#s",
            "#c",
            "52$i",
        ]
    );
    assert_eq!(sim.receipts(), 0);
}

#[derive(Debug)]
struct ByteReads(Arc<dyn Link>);

impl Link for ByteReads {
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Ok(Box::new(BytePort(self.0.open(port, baud_rate)?)))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.0.candidates()
    }

    fn time_scale(&self) -> u32 {
        self.0.time_scale()
    }
}

struct BytePort(Box<dyn Port>);

impl Port for BytePort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.0.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.0.read(&mut buffer[..1], timeout)
    }
}

#[test]
fn a_resumed_ack_gets_a_fresh_receive_deadline_without_another_status_poll() {
    let sim = Simulator::start();
    supported(&sim);
    sim.fault("#r", Fault::WaitPolls(1));
    let driver = Printer::new(Arc::new(ByteReads(sim.link())), connection(&sim));
    let mut log = Log::default();
    driver
        .print_report(Report::Daily, "ABC1234567890", &mut log)
        .unwrap();
    assert_eq!(
        log.waits,
        [
            Some("Brak papieru lub niskie napięcie akumulatora".to_owned()),
            None
        ]
    );
    let frames = sim.frames();
    let report = frames
        .iter()
        .position(|raw| frame::decode(raw).is_ok_and(|body| body == b"1;26;10;24#r"))
        .unwrap();
    assert_eq!(&frames[report + 1..], [vec![DLE2], vec![super::DLE]]);
    assert_eq!(log.commands, ["#r"]);
    assert_eq!(sim.reports(), 1);
}
