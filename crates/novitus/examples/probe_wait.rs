//! TEMPORARY probe: count operator-wait notifications during one paper-out wait.
use std::io;

use fiscal::{driver::Driver, journal::Journal, link::Connection, report::Report};
use novitus::{
    printer::Printer,
    simulator::{Fault, Simulator},
};

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
        // Operator fixes the paper only after the third notification.
        if self.events.iter().filter(|event| event.is_some()).count() == 3 {
            self.sim.resume();
        }
    }
}

fn main() {
    let sim = Simulator::start();
    sim.set_identity("NOVITUS HD II ONLINE", "1.40");
    sim.fault("#r", Fault::Wait);
    let mut journal = Operator {
        sim: &sim,
        events: vec![],
    };
    let result = Printer::new(
        sim.link(),
        Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    )
    .print_report(Report::Daily, "ABC1234567890", &mut journal);
    println!("result: {result:?}");
    println!("events: {:?}", journal.events);
    println!("reports: {}", sim.reports());
}
