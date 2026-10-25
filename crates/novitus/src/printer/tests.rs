use std::io;

use fiscal::{
    cp1250,
    device::VatKind,
    driver::Driver,
    error::Error,
    journal::Journal,
    link::Connection,
    money::Grosze,
    receipt::{Advance, Line, Payment, PaymentForm, Quantity, Receipt, VatSlot},
    report::Report,
};

use super::{Printer, identify};
use crate::{
    frame, profile, receipt,
    session::{Session, identifier},
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
    assert!(matches!(
        driver.print_report(Report::Daily, "ABC1234567890", &mut log),
        Err(Error::NotReady(_))
    ));
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
const ADVANCE_REFUSAL: &str = "Drukarka nie obsługuje rozliczenia zaliczek.";
/// Names the fix: the printer header must contain the Holuto company name.
const FOOTER_REFUSAL: &str = "Nagłówek drukarki nie zawiera nazwy firmy z Holuto, a ta drukarka \
                              nie drukuje jej w stopce. Wpisz w Holuto nazwę zgodną z nagłówkiem \
                              drukarki.";

#[test]
fn identify_accepts_supported_models_only() {
    for (model, firmware) in [
        ("NOVITUS HD II ONLINE", "1.40"),
        ("NOVITUS DEON ONLINE", "1.40"),
        ("NOVITUS BONO ONLINE", "1.10"),
        ("NOVITUS POINT", "1.0"),
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, firmware);
        let device = identify(sim.link().as_ref(), &connection(&sim))
            .unwrap()
            .unwrap();
        assert!(device.ready(), "{}", device.description());
        assert_eq!(device.unique_number, "ABC1234567890");
    }
    for (model, firmware) in [
        ("POSNET TEMO ONLINE", "1.40"),
        ("NOVITUS UNKNOWN", "1.40"),
        ("NOVITUS HD II ONLINE", "99.00"),
    ] {
        let sim = Simulator::start();
        sim.set_identity(model, firmware);
        let found = identify(sim.link().as_ref(), &connection(&sim)).unwrap();
        assert!(
            found.is_none_or(|device| !device.ready()),
            "unqualified identity"
        );
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
fn non_native_dialect_is_not_ready() {
    for option in ["0", "2"] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_dialect(option);
        assert!(!printer(&sim).probe().unwrap().ready());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
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
fn receipt_frames_match_snapshot() {
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
    let frames = fiscal_bodies(&sim);
    insta::assert_debug_snapshot!(frames);
    assert_eq!(
        log.commands,
        [
            "$h", "$z", "$l", "$l", "$l", "$b", "$b", "$b", "$b", "$b", "$y", "$z", "$z"
        ]
    );
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn lost_reply_after_begin_is_uncertain() {
    for command in ["$h", "$l", "$b", "$y", "$z"] {
        let sim = Simulator::start();
        supported(&sim);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault(command, Fault::LoseReply);
        assert!(
            matches!(
                driver.print_receipt(&receipt(vec![]), &expected, &mut Log::default()),
                Err(Error::Uncertain(_))
            ),
            "{command}"
        );
        assert_eq!(
            fiscal_bodies(&sim)
                .iter()
                .filter(|body| body.as_str() == "0$h")
                .count(),
            1
        );
    }
}

#[test]
fn begin_refusal_is_certain_failure() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.fault("$h", Fault::Refuse(40));
    assert!(matches!(
        driver.print_receipt(&receipt(vec![]), &expected, &mut Log::default()),
        Err(Error::Device { code: 40, .. })
    ));
    assert_eq!(sim.receipts(), 0);
}

#[cfg(not(workshop_acceptance))]
#[test]
fn unqualified_advance_is_refused_before_begin() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let receipt = receipt(vec![Advance {
        vat: VatSlot::new(0).unwrap(),
        amount: Grosze::new(500).unwrap(),
    }]);
    assert!(matches!(
        driver.print_receipt(&receipt, &expected, &mut Log::default()),
        Err(Error::Invalid(_))
    ));
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
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
fn preflight_rejects_busy_readonly_missing_header_bad_clock_and_codepage() {
    for problem in 0..7 {
        let sim = Simulator::start();
        supported(&sim);
        match problem {
            0 => sim.set_status(0x74, 0x6F, "@s0;0;0;00000000;"),
            1 => sim.set_status(0x76, 0x6D, "@s0;0;0;00000000;"),
            2 => sim.set_status(0x74, 0x6D, "@s0;0;0;00000001;"),
            3 => sim.set_fiscal_mode(2),
            4 => sim.set_header(""),
            5 => sim.set_clock("26;2;30;12;0;0"),
            _ => sim.set_config_option(92, "0"),
        }
        let driver = printer(&sim);
        let result = driver.probe();
        assert!(
            result.map_or(true, |device| !device.ready()),
            "case {problem}"
        );
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
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
    }
}

#[test]
fn journal_failure_prevents_send_and_is_uncertain_only_after_begin() {
    for fail_at in [0, 1] {
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
        assert_eq!(fiscal_bodies(&sim).len(), fail_at);
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
    // The exact uninterrupted sequence: no step replayed, skipped or reordered.
    let clean = Simulator::start();
    supported(&clean);
    let clean_driver = printer(&clean);
    let clean_expected = clean_driver.probe().unwrap();
    let clean_printed = clean_driver
        .print_receipt(&receipt(vec![]), &clean_expected, &mut Log::default())
        .unwrap();
    assert_eq!(clean_printed.receipt_number, printed.receipt_number);
    assert_eq!(fiscal_bodies(&sim), fiscal_bodies(&clean));
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
fn unqualified_footer_is_refused_before_begin() {
    let sim = Simulator::start();
    supported(&sim);
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    sim.set_header("Inny podatnik");
    assert!(matches!(
        driver.print_receipt(&receipt(vec![]), &expected, &mut Log::default()),
        Err(Error::Invalid(text)) if text == FOOTER_REFUSAL
    ));
    assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
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
fn every_post_begin_stage_failure_is_uncertain_even_after_final_print() {
    for (body, receipts) in [
        ("100;1$z1234567890\r", 0),
        ("1$lNaprawa\r1\rA/10/10/", 0),
        ("1;9$b12/\r", 0),
        ("0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r60/0/0/60/0/", 0),
        ("12$z", 0),
        ("28;0$z", 1),
    ] {
        for fault in [Fault::LoseReply, Fault::CorruptReply, Fault::Disconnect] {
            let sim = Simulator::start();
            supported(&sim);
            let driver = printer(&sim);
            let expected = driver.probe().unwrap();
            sim.fault(body, fault);
            assert!(
                matches!(
                    driver.print_receipt(&receipt(vec![]), &expected, &mut Log::default()),
                    Err(Error::Uncertain(_))
                ),
                "{body:?}"
            );
            assert_eq!(sim.receipts(), receipts);
            assert_eq!(
                fiscal_bodies(&sim)
                    .iter()
                    .filter(|sent| *sent == body)
                    .count(),
                1
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
        if problem == 2 {
            assert!(matches!(
                result,
                Err(Error::Invalid(message))
                    if message == "Drukarka lub stawki VAT zmieniły się. Odśwież połączenie."
            ));
        } else {
            assert!(matches!(
                result,
                Err(Error::Invalid(_) | Error::NotReady(_))
            ));
        }
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn ambiguous_or_impossible_warsaw_time_never_prints() {
    for clock in [
        "26;10;25;2;30;0",
        "26;3;29;2;30;0",
        "26;2;30;12;0;0",
        "50;1;1;12;0;0",
    ] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_clock(clock);
        assert!(matches!(printer(&sim).probe(), Err(Error::Protocol(_))));
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}

#[test]
fn experimental_advance_allocation_and_footer_are_exact_and_gated() {
    let sim = Simulator::start();
    supported(&sim);
    let mut session = Session::open(sim.link().as_ref(), &connection(&sim)).unwrap();
    let mut caps = profile::lookup("NOVITUS HD II ONLINE", "1.40").unwrap();
    caps.advance_deductions = true;
    caps.company_footer = true;
    caps.allowed_characters.push_str("Łóź");
    let caps = &caps;
    let lines = [("Pierwszy", 1000), ("Drugi", 2000)]
        .map(|(name, value)| {
            Line::new(
                name,
                Quantity::parse("1").unwrap(),
                Grosze::new(value).unwrap(),
                VatSlot::new(0).unwrap(),
                caps,
            )
            .unwrap()
        })
        .to_vec();
    let job = Receipt::new(
        lines,
        vec![Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(1500).unwrap(),
        }],
        vec![
            Payment {
                form: PaymentForm::Cash,
                amount: Grosze::new(1500).unwrap(),
            },
            Payment {
                form: PaymentForm::Mobile,
                amount: Grosze::ZERO,
            },
        ],
        None,
        "Łódź",
        caps,
    )
    .unwrap();
    let steps = receipt::steps(&job, Some("Łódź"), caps).unwrap();
    super::dispatch(&mut session, &steps, &mut Log::default()).unwrap();
    assert_eq!(
        fiscal_bodies(&sim),
        [
            "0;1$hŁódź\r",
            "1$lPierwszy\r1\rA/10/10/",
            "1;@zPierwszy\rA\r10/0/",
            "2$lDrugi\r1\rA/20/20/",
            "1;@zDrugi\rA\r5/15/",
            "1;0$b15/\r",
            "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r15/0/0/15/0/",
            "12$z",
            "28;0$z"
        ]
    );
    assert!(
        sim.frames()
            .iter()
            .any(|raw| raw.windows(4).any(|part| part == [0xA3, 0xF3, b'd', 0x9F])),
        "footer is CP1250, not UTF-8"
    );
    assert_eq!(sim.receipts(), 1);
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
    assert_eq!(journal.log.len(), 13);
}

#[test]
fn waiting_cap_and_unexplained_silence_never_resend_or_guess_dle2_order() {
    for fault in [Fault::Wait, Fault::LoseReply] {
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
        if journal.waits.is_empty() {
            continue;
        }
        assert_eq!(journal.waits.last(), Some(&None));
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

#[cfg(not(workshop_acceptance))]
#[test]
fn zero_total_is_disabled_in_qualified_profiles_but_encodable_when_enabled() {
    let sim = Simulator::start();
    supported(&sim);
    let mut caps = profile::lookup("NOVITUS HD II ONLINE", "1.40").unwrap();
    let job = receipt(vec![
        Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(1000).unwrap(),
        },
        Advance {
            vat: VatSlot::new(1).unwrap(),
            amount: Grosze::new(2000).unwrap(),
        },
        Advance {
            vat: VatSlot::new(4).unwrap(),
            amount: Grosze::new(3000).unwrap(),
        },
    ]);
    assert!(matches!(
        receipt::steps(&job, None, &caps),
        Err(Error::Invalid(_))
    ));
    caps.advance_deductions = true;
    assert!(matches!(
        receipt::steps(&job, None, &caps),
        Err(Error::Invalid(_))
    ));
    caps.zero_total_receipts = true;
    let steps = receipt::steps(&job, None, &caps).unwrap();
    assert!(!steps.iter().any(|step| step.command == "$b"));
    let mut session = Session::open(sim.link().as_ref(), &connection(&sim)).unwrap();
    super::dispatch(&mut session, &steps, &mut Log::default()).unwrap();
    assert_eq!(sim.receipts(), 1);
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
