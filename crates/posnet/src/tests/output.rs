use jiff::civil::date;

use super::{Recorder, printer, sale, simulator};
use crate::{
    capabilities::capabilities,
    simulator::{SERIAL, Simulator},
    testing::{BRAND, line, pay},
};
use fiscal::{
    driver::Driver,
    money::Grosze,
    receipt::{Advance, PaymentForm, Receipt, VatSlot},
    report::Report,
};

fn stream(sim: &Simulator, from: usize) -> String {
    sim.frames()[from..]
        .iter()
        .map(|frame| {
            frame
                .iter()
                .flat_map(|byte| byte.escape_ascii())
                .map(char::from)
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn receipt_stream(receipt: &Receipt, header: &str) -> String {
    let sim = simulator();
    sim.set_header(header);
    let device = printer(&sim);
    let status = device.probe().unwrap();
    let from = sim.frames().len();
    let mut journal = Recorder::default();
    let printed = device
        .print_receipt(receipt, &status, &mut journal)
        .unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(printed.issued_at, "2026-07-01T10:34:00Z".parse().unwrap());
    assert_eq!(sim.receipts()[0].total, receipt.fiscal_total().get());
    let frames = stream(&sim, from);
    assert!(
        device.probe().unwrap().ready(),
        "receipt and optional footer are closed"
    );
    frames
}

#[test]
fn sale_cash_frames() {
    insta::assert_snapshot!(receipt_stream(&sale(), "&cWARSZTAT KOWALSKI&c"));
}

#[test]
fn mixed_vat_and_fractional_quantity_frames() {
    let receipt = Receipt::new(
        vec![
            line("Olej łódź", "0.5", "19.99", 0),
            line("Usługa", "1", "2", 1),
            line("Towar", "1", "3", 2),
            line("Zwolnione", "1", "4", 6),
        ],
        vec![],
        vec![pay(PaymentForm::Cash, 1900)],
        None,
        BRAND,
        &capabilities(),
    )
    .unwrap();
    insta::assert_snapshot!(receipt_stream(&receipt, BRAND));
}

#[test]
fn buyer_nip_frames() {
    let receipt = Receipt::new(
        vec![line("Olej", "1", "1", 0)],
        vec![],
        vec![pay(PaymentForm::Cash, 100)],
        Some("123-456-78-90".to_owned()),
        BRAND,
        &capabilities(),
    )
    .unwrap();
    insta::assert_snapshot!(receipt_stream(&receipt, BRAND));
}

#[test]
fn advance_deductions_and_split_payment_frames() {
    let receipt = Receipt::new(
        vec![line("Olej", "2", "1", 0), line("Usługa", "1", "3", 1)],
        vec![
            Advance {
                vat: VatSlot::new(0).unwrap(),
                amount: Grosze::new(100).unwrap(),
            },
            Advance {
                vat: VatSlot::new(1).unwrap(),
                amount: Grosze::new(150).unwrap(),
            },
        ],
        vec![pay(PaymentForm::Cash, 100), pay(PaymentForm::Card, 150)],
        None,
        BRAND,
        &capabilities(),
    )
    .unwrap();
    insta::assert_snapshot!(receipt_stream(&receipt, BRAND));
}

macro_rules! payment_frames {
    ($name:ident,$form:expr) => {
        #[test]
        fn $name() {
            let receipt = Receipt::new(
                vec![line("Olej", "1", "1", 0)],
                vec![],
                vec![pay($form, 100), pay(PaymentForm::Cash, 0)],
                None,
                BRAND,
                &capabilities(),
            )
            .unwrap();
            insta::assert_snapshot!(receipt_stream(&receipt, BRAND));
        }
    };
}
payment_frames!(card_frames, PaymentForm::Card);
payment_frames!(transfer_frames, PaymentForm::Transfer);
payment_frames!(mobile_frames, PaymentForm::Mobile);
payment_frames!(credit_frames, PaymentForm::Credit);

#[test]
fn fully_prepaid_sale_frames() {
    let receipt = Receipt::new(
        vec![line("Olej", "1", "1", 0)],
        vec![Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(100).unwrap(),
        }],
        vec![pay(PaymentForm::Cash, 0)],
        None,
        BRAND,
        &capabilities(),
    )
    .unwrap();
    insta::assert_snapshot!(receipt_stream(&receipt, BRAND));
}

#[test]
fn brand_footer_frames() {
    insta::assert_snapshot!(receipt_stream(&sale(), "&cInny warsztat&c"));
}

macro_rules! report_frames {
    ($name:ident,$report:expr) => {
        #[test]
        fn $name() {
            let sim = simulator();
            printer(&sim)
                .print_report($report, SERIAL, &mut Recorder::default())
                .unwrap();
            insta::assert_snapshot!(stream(&sim, 0));
            assert_eq!(sim.reports().len(), 1);
        }
    };
}
report_frames!(daily_report_frames, Report::Daily);
report_frames!(
    monthly_report_frames,
    Report::Monthly {
        year: 2026,
        month: 6
    }
);
report_frames!(
    periodic_report_frames,
    Report::Periodic {
        from: date(2026, 1, 1),
        to: date(2026, 6, 30),
        summary: false
    }
);
report_frames!(
    periodic_summary_frames,
    Report::Periodic {
        from: date(2026, 1, 1),
        to: date(2026, 6, 30),
        summary: true
    }
);

#[test]
fn fully_prepaid_sale_with_brand_footer_frames() {
    let receipt = Receipt::new(
        vec![line("Olej", "1", "1", 0)],
        vec![Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(100).unwrap(),
        }],
        vec![],
        None,
        BRAND,
        &capabilities(),
    )
    .unwrap();
    insta::assert_snapshot!(receipt_stream(&receipt, "Inny warsztat"));
}
