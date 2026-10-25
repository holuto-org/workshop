//! Throwaway probe: timeout handed to the port for each fiscal frame (time scale 1).
use elzab::{frame, printer::Printer, profile, simulator::Simulator};
use fiscal::{
    cp1250,
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
    sync::{Arc, Mutex},
    time::Duration,
};

type Seen = Arc<Mutex<Vec<(String, Duration)>>>;
#[derive(Debug)]
struct Recording(Arc<dyn Link>, Seen);
impl Link for Recording {
    fn open(&self, port: &str, baud: u32) -> Result<Box<dyn Port>, Error> {
        Ok(Box::new(RecPort(self.0.open(port, baud)?, Arc::clone(&self.1))))
    }
    fn candidates(&self) -> Vec<Candidate> {
        self.0.candidates()
    }
    fn time_scale(&self) -> u32 {
        1
    }
}
struct RecPort(Box<dyn Port>, Seen);
impl Port for RecPort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        let body = cp1250::decode(frame::decode(bytes).unwrap().1).unwrap();
        let command = body.split('\u{1d}').next().unwrap().to_owned();
        self.1.lock().unwrap().push((command, timeout));
        self.0.write_all(bytes, timeout)
    }
    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.0.read(buffer, timeout)
    }
}
struct Log;
impl Journal for Log {
    fn dispatching(&mut self, _: &'static str) -> io::Result<()> {
        Ok(())
    }
    fn waiting(&mut self, _: Option<&str>) {}
}

fn main() {
    let sim = Simulator::start();
    let seen = Seen::default();
    let link = Arc::new(Recording(sim.link(), Arc::clone(&seen)));
    let printer = Printer::new(
        link,
        Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    );
    let expected = printer.probe().unwrap();
    let caps = profile::lookup("ELZAB CUBE online").unwrap().capabilities;
    let line = Line::new(
        "Olej",
        Quantity::parse("1").unwrap(),
        Grosze::new(2000).unwrap(),
        VatSlot::new(0).unwrap(),
        &caps,
    )
    .unwrap();
    let receipt = Receipt::new(
        vec![line],
        vec![Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(500).unwrap(),
        }],
        vec![Payment {
            form: PaymentForm::Cash,
            amount: Grosze::new(1500).unwrap(),
        }],
        Some("1234567890".to_owned()),
        "Warsztat",
        &caps,
    )
    .unwrap();
    printer.print_receipt(&receipt, &expected, &mut Log).unwrap();
    printer
        .print_report(Report::Monthly { year: 2026, month: 9 }, &expected.unique_number, &mut Log)
        .unwrap();
    let daily = printer.print_report(Report::Daily, &expected.unique_number, &mut Log);
    println!("daily: {}", daily.unwrap_err());
    for (command, timeout) in seen.lock().unwrap().iter() {
        if command.starts_with('L') && command != "LPC" || command.starts_with('R') && command != "RNR" {
            println!("{command}: {timeout:?}");
        }
    }
}
