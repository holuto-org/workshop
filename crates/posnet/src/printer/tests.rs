use std::{collections::BTreeMap, io, sync::Arc};

use jiff::civil::date;
use serde_json::json;

use super::{Journal, Printed, Printer, Report, Status};
use crate::{
    Error,
    frame::encode,
    receipt::{
        Advance, PaymentForm, Receipt, VatSlot,
        tests::{BRAND, line, pay},
    },
    session::{Connection, Link},
    simulator::{Fault, SERIAL, Simulator},
};

/// Records every journaled command; fails from the `fail_at`-th entry on.
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

fn params(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
    fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn reply(fields: &[(&str, &str)]) -> Fault {
    Fault::Reply(
        fields
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    )
}

fn simple_receipt(brand: &str) -> Receipt {
    Receipt::new(
        vec![line("Olej", "1", "1.00", 0)],
        vec![],
        vec![pay(PaymentForm::Cash, 100)],
        None,
        brand,
    )
    .unwrap()
}

fn print(sim: &Simulator, receipt: &Receipt) -> Result<Printed, Error> {
    let printer = printer(sim);
    let status = printer.probe()?;
    printer.print_receipt(receipt, &status, &mut Recorder::default())
}

#[test]
fn brand_footer_only_when_the_header_lacks_the_company_name() {
    let sim = Simulator::start();
    let printed = print(&sim, &simple_receipt("warsztat kowalski")).unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(sim.footers().len(), 0);

    sim.set_header("&cInny Serwis&c");
    let printed = print(&sim, &simple_receipt("Warsztat Kowalski")).unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("2"));
    assert_eq!(
        sim.footers(),
        [params(&[("id", "25"), ("na", "Warsztat Kowalski")])]
    );
    assert!(!sim.transaction_open());
    assert!(probe_is_ready(&sim), "the footer was closed");
}

fn probe_is_ready(sim: &Simulator) -> bool {
    printer(sim).probe().unwrap().ready()
}

#[test]
fn probe_json_keeps_rates_as_two_place_percent_strings() {
    let sim = Simulator::start();
    sim.set_vat(["23,00", "8,00", "5,50", "0,00", "101,00", "99,99", "100,00"]);
    let status = serde_json::to_value(printer(&sim).probe().unwrap()).unwrap();
    let rates = ["23.00", "8.00", "5.50", "0.00", "101.00", "99.99", "100.00"];
    let expected: Vec<_> = (0..)
        .zip(rates)
        .map(|(slot, percent)| json!({"slot": slot, "percent": percent}))
        .collect();
    assert_eq!(status["vat"], json!(expected));
}

#[test]
fn receipt_preserves_line_slots_buyer_nip_and_each_payment_form() {
    for (form, wire_type) in [
        (PaymentForm::Cash, "0"),
        (PaymentForm::Card, "2"),
        (PaymentForm::Transfer, "8"),
        (PaymentForm::Credit, "5"),
        (PaymentForm::Mobile, "6"),
    ] {
        for nip in [None, Some("5222628262")] {
            let sim = Simulator::start();
            let lines = vec![
                line("Usługa", "1", "10.00", 0),
                line("Część", "2", "2.50", 1),
                line("Sprzedaż zero", "1", "3.00", 2),
                line("Zwolniona", "1", "2.00", 6),
            ];
            let receipt = Receipt::new(
                lines,
                vec![],
                vec![pay(form, 2000)],
                nip.map(str::to_owned),
                BRAND,
            )
            .unwrap();
            let mut recorder = Recorder::default();
            let device = printer(&sim);
            let status = device.probe().unwrap();
            let outcome = device
                .print_receipt(&receipt, &status, &mut recorder)
                .unwrap();
            assert_eq!(outcome.receipt_number.as_deref(), Some("1"));
            let [printed] = sim.receipts().try_into().unwrap();
            assert_eq!(printed.total, 2000);
            assert_eq!(printed.buyer_nip.as_deref(), nip);
            let mut payment = params(&[("ty", wire_type), ("wa", "2000")]);
            if form == PaymentForm::Mobile {
                drop(payment.insert("na".to_owned(), "Płatność mobilna".to_owned()));
            }
            assert_eq!(printed.payments, [payment]);
            assert_eq!(
                printed.lines,
                [
                    params(&[
                        ("na", "Usługa"),
                        ("vt", "0"),
                        ("pr", "1000"),
                        ("il", "1"),
                        ("wa", "1000")
                    ]),
                    params(&[
                        ("na", "Część"),
                        ("vt", "1"),
                        ("pr", "250"),
                        ("il", "2"),
                        ("wa", "500")
                    ]),
                    params(&[
                        ("na", "Sprzedaż zero"),
                        ("vt", "2"),
                        ("pr", "300"),
                        ("il", "1"),
                        ("wa", "300")
                    ]),
                    params(&[
                        ("na", "Zwolniona"),
                        ("vt", "6"),
                        ("pr", "200"),
                        ("il", "1"),
                        ("wa", "200")
                    ]),
                ]
            );
            let first = if nip.is_some() {
                vec!["trinit", "trnipset"]
            } else {
                vec!["trinit"]
            };
            assert_eq!(recorder.stages.get(..first.len()), Some(first.as_slice()));
            assert_eq!(recorder.stages.last(), Some(&"trend"));
        }
    }
}

#[test]
fn buyer_nip_does_not_leak_into_the_next_receipt() {
    let sim = Simulator::start();
    let with_nip = Receipt::new(
        vec![line("Olej", "1", "1.00", 0)],
        vec![],
        vec![pay(PaymentForm::Cash, 100)],
        Some("5222628262".to_owned()),
        BRAND,
    )
    .unwrap();
    let _first = print(&sim, &with_nip).unwrap();
    let _second = print(&sim, &simple_receipt(BRAND)).unwrap();
    let nips: Vec<_> = sim
        .receipts()
        .into_iter()
        .map(|receipt| receipt.buyer_nip)
        .collect();
    assert_eq!(nips, [Some("5222628262".to_owned()), None]);
}

#[test]
fn a_failure_mid_receipt_is_uncertain_and_journaled_at_the_failed_command() {
    for failed in ["trnipset", "trpayment", "trend"] {
        let sim = Simulator::start();
        sim.fault(failed, Fault::Error);
        let receipt = Receipt::new(
            vec![line("Wymiana oleju", "1", "100.00", 0)],
            vec![],
            vec![pay(PaymentForm::Cash, 10_000)],
            Some("5222628262".to_owned()),
            BRAND,
        )
        .unwrap();
        let printer = printer(&sim);
        let status = printer.probe().unwrap();
        let mut recorder = Recorder::default();
        let error = printer
            .print_receipt(&receipt, &status, &mut recorder)
            .unwrap_err();
        assert!(matches!(error, Error::Uncertain(_)), "{error:?}");
        assert!(error.to_string().contains("Nie wysyłaj jej ponownie"));
        assert_eq!(recorder.stages.last(), Some(&failed));
        assert_eq!(sim.receipts().len(), 0);
    }
}

#[test]
fn a_journal_failure_stops_the_command_it_would_have_recorded() {
    let sim = Simulator::start();
    let printer = printer(&sim);
    let status = printer.probe().unwrap();
    let mut first = Recorder {
        fail_at: Some(0),
        ..Recorder::default()
    };
    let error = printer
        .print_receipt(&simple_receipt(BRAND), &status, &mut first)
        .unwrap_err();
    assert!(
        matches!(error, Error::Journal(_)),
        "nothing was sent: {error:?}"
    );
    let trinit = encode("trinit", &[("bm", "0".to_owned())]).unwrap();
    assert!(!sim.frames().contains(&trinit));

    let mut later = Recorder {
        fail_at: Some(1),
        ..Recorder::default()
    };
    let error = printer
        .print_receipt(&simple_receipt(BRAND), &status, &mut later)
        .unwrap_err();
    assert!(
        matches!(error, Error::Uncertain(_)),
        "trinit was sent: {error:?}"
    );
    assert_eq!(sim.receipts().len(), 0);
}
/// A command, the device's answer to it and the problem the status must name.
type Case = (
    &'static str,
    Vec<(&'static str, &'static str)>,
    &'static str,
);

#[test]
fn preflight_refuses_every_documented_device_problem() {
    let inactive = ["va", "vb", "vc", "vd", "ve", "vf", "vg"].map(|key| (key, "101,00"));
    let cases: [Case; 8] = [
        (
            "getrealid",
            vec![("nm", "POSNET THERMAL HD"), ("vr", "1"), ("nu", "X")],
            "nie jest drukarką Temo",
        ),
        (
            "scomm",
            vec![("fs", "0"), ("hr", "1"), ("ts", "0")],
            "trybie fiskalnym",
        ),
        (
            "scomm",
            vec![("fs", "1"), ("hr", "0"), ("ts", "0")],
            "zaprogramowanego nagłówka",
        ),
        (
            "sdev",
            vec![("ds", "1"), ("qe", "1")],
            "oczekuje na operatora",
        ),
        (
            "sdev",
            vec![("ds", "0"), ("qe", "0")],
            "oczekujące polecenia",
        ),
        ("sprn", vec![("pr", "5")], "Brak papieru"),
        (
            "strns",
            vec![("to", "1"), ("fe", "0")],
            "Otwarta transakcja",
        ),
        ("vatget", inactive.to_vec(), "stawek"),
    ];
    for (command, answer, problem) in cases {
        let sim = Simulator::start();
        sim.fault(command, reply(&answer));
        let printer = printer(&sim);
        let status = printer.probe().unwrap();
        assert!(
            !status.ready() && status.description().contains(problem),
            "{command}: {status:?}"
        );
        let error = printer
            .print_receipt(&simple_receipt(BRAND), &status, &mut Recorder::default())
            .unwrap_err();
        assert!(
            matches!(&error, Error::NotReady(message) if message.contains(problem)),
            "{error:?}"
        );
        assert_eq!(sim.receipts().len(), 0);
    }
}

#[test]
fn a_garbled_status_reply_is_an_error_never_a_ready_printer() {
    let vat = |value| ["va", "vb", "vc", "vd", "ve", "vf", "vg"].map(|key| (key, value));
    let cases: [(&str, Vec<(&str, &str)>); 4] = [
        ("getrealid", vec![("vr", "32.01"), ("nu", "DEMO0000001")]),
        ("scomm", vec![("fs", "maybe"), ("hr", "1"), ("ts", "0")]),
        ("vatget", vat("-").to_vec()),
        ("vatget", vat("Infinity").to_vec()),
    ];
    for (command, answer) in cases {
        let sim = Simulator::start();
        sim.fault(command, reply(&answer));
        assert!(
            matches!(printer(&sim).probe(), Err(Error::Protocol(_))),
            "{command}"
        );
    }
}

#[test]
fn receipt_refuses_an_inactive_rate_and_one_that_changed_since_the_probe() {
    let sim = Simulator::start();
    let printer = printer(&sim);
    let status = printer.probe().unwrap();
    let on_unused_slot = Receipt::new(
        vec![
            line("Olej", "1", "1.00", 0),
            line("Inna usługa", "1", "1.00", 3),
        ],
        vec![],
        vec![pay(PaymentForm::Cash, 200)],
        None,
        BRAND,
    )
    .unwrap();
    let error = printer
        .print_receipt(&on_unused_slot, &status, &mut Recorder::default())
        .unwrap_err();
    assert!(error.to_string().contains("nieaktywna"), "{error}");

    sim.set_vat([
        "8,00", "8,00", "0,00", "101,00", "101,00", "101,00", "100,00",
    ]);
    let error = printer
        .print_receipt(&simple_receipt(BRAND), &status, &mut Recorder::default())
        .unwrap_err();
    assert!(error.to_string().contains("zmieniła się"), "{error}");
    assert_eq!(sim.receipts().len(), 0);
}

#[test]
fn a_different_device_on_the_port_is_refused() {
    let sim = Simulator::start();
    let printer = printer(&sim);
    let mut paired = printer.probe().unwrap();
    paired.serial = "OTHER".to_owned();
    let error = printer
        .print_receipt(&simple_receipt(BRAND), &paired, &mut Recorder::default())
        .unwrap_err();
    assert!(error.to_string().contains("OTHER"), "{error}");
    assert!(printer.verify("OTHER").is_err());
    assert!(
        printer
            .print_report(Report::Daily, "OTHER", &mut Recorder::default())
            .is_err()
    );
    assert_eq!((sim.receipts().len(), sim.reports().len()), (0, 0));
}

#[test]
fn advances_and_split_payments_use_the_exact_wire_frames() {
    let sim = Simulator::start();
    let receipt = Receipt::new(
        vec![
            line("Usługa", "1", "10.00", 0),
            line("Część", "1", "5.00", 1),
        ],
        vec![
            Advance {
                vat: VatSlot::new(0).unwrap(),
                amount: 500.try_into().unwrap(),
            },
            Advance {
                vat: VatSlot::new(1).unwrap(),
                amount: 100.try_into().unwrap(),
            },
        ],
        vec![
            pay(PaymentForm::Cash, 100),
            pay(PaymentForm::Card, 200),
            pay(PaymentForm::Transfer, 100),
            pay(PaymentForm::Mobile, 200),
            pay(PaymentForm::Credit, 300),
        ],
        None,
        BRAND,
    )
    .unwrap();
    let printed = print(&sim, &receipt).unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    let frame = |command, fields: &[(&str, &str)]| {
        encode(
            command,
            &fields
                .iter()
                .map(|(key, value)| (*key, (*value).to_owned()))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    let frames = sim.frames();
    let start = frames
        .iter()
        .position(|sent| *sent == frame("trinit", &[("bm", "0")]))
        .unwrap()
        + 3;
    assert_eq!(
        frames[start..frames.len() - 1],
        [
            frame(
                "tradvance",
                &[
                    ("na", "Zaliczka"),
                    ("vt", "0"),
                    ("pr", "500"),
                    ("wa", "1000")
                ]
            ),
            frame(
                "tradvance",
                &[
                    ("na", "Zaliczka"),
                    ("vt", "1"),
                    ("pr", "100"),
                    ("wa", "500")
                ]
            ),
            frame("trpayment", &[("ty", "0"), ("wa", "100")]),
            frame("trpayment", &[("ty", "2"), ("wa", "200")]),
            frame("trpayment", &[("ty", "8"), ("wa", "100")]),
            frame(
                "trpayment",
                &[("ty", "6"), ("wa", "200"), ("na", "Płatność mobilna")]
            ),
            frame("trpayment", &[("ty", "5"), ("wa", "300")]),
            frame("trend", &[("to", "900"), ("fp", "900")]),
        ]
    );
    assert_eq!(sim.receipts()[0].total, 900);
}

#[test]
fn reports_send_the_documented_commands_and_refuse_a_reversed_range() {
    let sim = Simulator::start();
    let printer = printer(&sim);
    let mut recorder = Recorder::default();
    printer
        .print_report(Report::Daily, SERIAL, &mut recorder)
        .unwrap();
    printer
        .print_report(
            Report::Monthly {
                year: 2026,
                month: 8,
            },
            SERIAL,
            &mut recorder,
        )
        .unwrap();
    let periodic = Report::Periodic {
        from: date(2026, 8, 1),
        to: date(2026, 8, 31),
        summary: true,
    };
    printer
        .print_report(periodic, SERIAL, &mut recorder)
        .unwrap();
    let reversed = Report::Periodic {
        from: date(2026, 8, 31),
        to: date(2026, 8, 1),
        summary: false,
    };
    let error = printer
        .print_report(reversed, SERIAL, &mut recorder)
        .unwrap_err();
    assert!(error.to_string().contains("późniejsza"), "{error}");
    assert_eq!(
        recorder.stages,
        ["dailyrep", "monthlyrep", "periodicrepbydates"]
    );
    let reports: Vec<_> = sim
        .reports()
        .into_iter()
        .map(|report| (report.command, report.params))
        .collect();
    // The daily report's date is the printer's own clock, not the PC's.
    let today = jiff::Zoned::now().date().to_string();
    assert_eq!(
        reports,
        [
            ("dailyrep".to_owned(), params(&[("da", &today)])),
            (
                "monthlyrep".to_owned(),
                params(&[("da", "2026-08-01"), ("su", "0")])
            ),
            (
                "periodicrepbydates".to_owned(),
                params(&[("fd", "2026-08-01"), ("td", "2026-08-31"), ("su", "1")])
            ),
        ]
    );
}

#[test]
fn verify_requires_the_same_ready_device() {
    let sim = Simulator::start();
    let printer = printer(&sim);
    assert_eq!(printer.verify(SERIAL).unwrap().serial, SERIAL);
    sim.set_transaction_open(true);
    let error = printer.verify(SERIAL).unwrap_err();
    assert!(
        matches!(&error, Error::NotReady(message) if message.contains("Otwarta")),
        "{error}"
    );
}

fn unconfigured(link: Arc<dyn Link>) -> Printer {
    Printer::new(
        link,
        Connection {
            port: String::new(),
            baud_rate: 9600,
        },
    )
}

#[test]
fn detect_adopts_the_port_that_identifies_as_a_temo() {
    let sim = Simulator::start();
    let mut printer = unconfigured(sim.link_with(
        vec!["/dev/null".to_owned(), sim.address().to_owned()],
        vec![],
    ));
    let status: Status = printer.detect(true).unwrap();
    assert_eq!(status.serial, SERIAL);
    assert_eq!(printer.connection().port, sim.address());
    assert!(
        printer.probe().unwrap().ready(),
        "the adopted port is now the configured one"
    );
}

#[test]
fn detect_reports_the_original_error_when_nothing_answers() {
    let sim = Simulator::start();
    let mut printer = unconfigured(sim.link_with(vec!["/dev/null".to_owned()], vec![]));
    assert!(matches!(printer.detect(true), Err(Error::NoPort)));
    assert_eq!(printer.connection().port, "");
}

#[test]
fn detect_names_the_port_the_os_refused_to_open() {
    let sim = Simulator::start();
    let mut printer = unconfigured(sim.link_with(
        vec!["/dev/null".to_owned(), "/dev/ttyACM0".to_owned()],
        vec!["/dev/ttyACM0".to_owned()],
    ));
    let error = printer.detect(true).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Brak uprawnień do portu /dev/ttyACM0")
            && error.to_string().contains("dialout"),
        "{error}"
    );
    assert_eq!(printer.connection().port, "");
}

#[test]
fn detect_never_switches_the_device_when_forbidden() {
    let sim = Simulator::start();
    let mut printer = unconfigured(sim.link_with(vec![sim.address().to_owned()], vec![]));
    assert!(matches!(printer.detect(false), Err(Error::NoPort)));
    assert_eq!(printer.connection().port, "");
    assert!(sim.frames().is_empty(), "no device was even asked");
}
