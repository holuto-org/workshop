use std::{io, time::Duration};

use super::{Journal, execute};
use crate::{
    Error, frame,
    receipt::Step,
    session::{Connection, Session},
    simulator::{Fault, Hold, Simulator},
};

#[derive(Clone, Copy)]
enum Action {
    Resume,
    CorruptQuery,
    LoseQuery,
    RefuseQuery,
    Stay,
    Flap,
}

struct Operator<'a> {
    sim: &'a Simulator,
    action: Action,
    heard: Vec<Option<String>>,
    hold: Option<Hold>,
}

impl Journal for Operator<'_> {
    fn dispatching(&mut self, _command: &'static str) -> io::Result<()> {
        Ok(())
    }

    fn waiting(&mut self, problem: Option<&str>) {
        self.heard.push(problem.map(str::to_owned));
        match (self.action, problem) {
            (Action::Resume, Some(_)) => {
                self.sim.set_paper(true);
                self.sim.set_status("pr", "0");
                self.sim.set_status("ds", "0");
                if let Some(hold) = &self.hold {
                    hold.release();
                }
            },
            (Action::CorruptQuery, Some(_)) => self.sim.fault("!sprn", Fault::CorruptReply),
            (Action::LoseQuery, Some(_)) => self.sim.fault("!sprn", Fault::LoseReply),
            (Action::RefuseQuery, Some(_)) => self.sim.fault("!sprn", Fault::Error),
            (Action::Flap, Some(_)) => self.sim.set_status("ds", "0"),
            (Action::Flap, None) if self.heard.len() < 18 => self.sim.set_status("ds", "2"),
            (
                Action::Stay
                | Action::Resume
                | Action::CorruptQuery
                | Action::LoseQuery
                | Action::RefuseQuery
                | Action::Flap,
                None,
            )
            | (Action::Stay, Some(_)) => {},
        }
    }
}

fn report(sim: &Simulator) -> (Session, Step) {
    let session = Session::open(
        sim.link().as_ref(),
        &Connection {
            port: sim.address().to_owned(),
            baud_rate: 9600,
        },
    )
    .unwrap();
    (
        session,
        Step {
            command: "dailyrep",
            params: vec![("da", "2026-07-01".to_owned())],
            timeout: Duration::from_secs(120),
        },
    )
}

#[test]
fn paper_cover_lever_temperature_and_key_waits_resume_without_resending() {
    for (field, value, message) in [
        ("pr", "1", "Podniesiona dźwignia"),
        ("pr", "3", "Otwarta pokrywa"),
        ("pr", "5", "Brak papieru"),
        ("pr", "6", "Nieprawidłowa temperatura lub zasilanie"),
        ("ds", "2", "Drukarka oczekuje na operatora"),
        ("ds", "3", "Drukarka oczekuje na operatora"),
    ] {
        let sim = Simulator::start();
        sim.set_status(field, value);
        let hold = sim.hold("dailyrep");
        let mut journal = Operator {
            sim: &sim,
            action: Action::Resume,
            heard: vec![],
            hold: Some(hold),
        };
        let (mut session, step) = report(&sim);
        let _reply = execute(&mut session, &step, &mut journal).unwrap();
        assert_eq!(
            journal.heard,
            [Some(message.to_owned()), None],
            "{field}={value}"
        );
        let fiscal: Vec<_> = sim
            .frames()
            .iter()
            .filter(|raw| frame::decode(raw).unwrap().0 == "dailyrep")
            .cloned()
            .collect();
        assert_eq!(fiscal, [frame::encode("dailyrep", &step.params).unwrap()]);
        assert_eq!(sim.reports()[0].command, "dailyrep");
    }
}

#[test]
fn every_error_exit_clears_the_operator_notice() {
    for action in [
        Action::CorruptQuery,
        Action::LoseQuery,
        Action::RefuseQuery,
        Action::Stay,
    ] {
        let sim = Simulator::start();
        sim.set_status("ds", "2");
        sim.fault("dailyrep", Fault::LoseReply);
        let (mut session, step) = report(&sim);
        let mut journal = Operator {
            sim: &sim,
            action,
            heard: vec![],
            hold: None,
        };
        assert!(
            execute(&mut session, &step, &mut journal).is_err(),
            "failed status query or operator cap"
        );
        assert_eq!(
            journal.heard,
            [Some("Drukarka oczekuje na operatora".to_owned()), None]
        );
        assert_eq!(sim.reports().len(), 1, "never execute the command twice");
    }
}

#[test]
fn ready_waiting_flaps_cannot_restart_the_ten_minute_operator_cap() {
    let sim = Simulator::start();
    sim.set_status("ds", "2");
    sim.fault("dailyrep", Fault::LoseReply);
    let (mut session, step) = report(&sim);
    let mut journal = Operator {
        sim: &sim,
        action: Action::Flap,
        heard: vec![],
        hold: None,
    };
    assert!(
        matches!(
            execute(&mut session, &step, &mut journal),
            Err(Error::Timeout("dailyrep"))
        ),
        "no response is never a success"
    );
    assert!(
        journal.heard.len() < 18,
        "flapping reset the operator cap: {:?}",
        journal.heard
    );
    assert_eq!(journal.heard.last(), Some(&None));
    assert_eq!(sim.reports().len(), 1);
}

#[test]
fn silence_without_a_wait_reason_and_unanswered_queries_remain_timeouts() {
    for unanswered in [false, true] {
        let sim = Simulator::start();
        sim.fault("dailyrep", Fault::LoseReply);
        if unanswered {
            sim.fault("!sdev", Fault::LoseReply);
        }
        let (mut session, step) = report(&sim);
        let mut journal = Operator {
            sim: &sim,
            action: Action::Stay,
            heard: vec![],
            hold: None,
        };
        assert!(
            matches!(
                execute(&mut session, &step, &mut journal),
                Err(Error::Timeout("dailyrep"))
            ),
            "silent command"
        );
        assert_eq!(journal.heard, Vec::<Option<String>>::new());
        assert_eq!(sim.reports().len(), 1);
    }
}
