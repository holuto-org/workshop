use std::{collections::VecDeque, io, time::Duration};

use fiscal::{
    error::Error,
    journal::Journal,
    link::{Connection, Port},
};

use super::execute;
use crate::{
    frame::{self, tests::reply},
    receipt::Step,
    session::Session,
    simulator::{Fault, Hold, SimReport, Simulator},
};

fn report_effect() -> SimReport {
    SimReport {
        command: "dailyrep".to_owned(),
        params: [("da".to_owned(), "2026-07-01".to_owned())].into(),
    }
}

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
        assert_eq!(sim.reports(), [report_effect()]);
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
        let error = execute(&mut session, &step, &mut journal).unwrap_err();
        if matches!(action, Action::CorruptQuery) {
            assert!(
                matches!(&error, Error::Protocol(message)
                    if message == "Uszkodzona odpowiedź drukarki (CRC lub kodowanie)."),
                "{error:?}"
            );
        } else {
            assert!(matches!(error, Error::Timeout("dailyrep")), "{error:?}");
        }
        assert_eq!(
            journal.heard,
            [Some("Drukarka oczekuje na operatora".to_owned()), None]
        );
        assert_eq!(
            sim.reports(),
            [report_effect()],
            "never execute the command twice"
        );
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
    assert_eq!(sim.reports(), [report_effect()]);
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
        assert_eq!(sim.reports(), [report_effect()]);
    }
}

struct LateReply {
    reads: VecDeque<Vec<u8>>,
    then: Vec<u8>,
    answer: Vec<u8>,
}

impl Port for LateReply {
    fn write_all(&mut self, bytes: &[u8], _timeout: Duration) -> io::Result<()> {
        if frame::decode(bytes).is_ok_and(|(command, _)| command == "!sdev") {
            self.reads.push_back(self.answer.clone());
            self.reads.push_back(self.then.clone());
        }
        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        let bytes = self.reads.pop_front().ok_or(io::ErrorKind::TimedOut)?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

struct Silent;

impl Journal for Silent {
    fn dispatching(&mut self, _command: &'static str) -> io::Result<()> {
        Ok(())
    }

    fn waiting(&mut self, problem: Option<&str>) {
        panic!("no operator wait was reported, got {problem:?}");
    }
}

#[test]
fn a_confirmed_reply_survives_a_damaged_or_stray_frame_that_follows_it() {
    for then in [b"\x02!sdev\tds\t0\t#0000\x03".to_vec(), reply("trend\t")] {
        let mut session = Session::new(Box::new(LateReply {
            reads: VecDeque::new(),
            then,
            answer: reply("dailyrep\t"),
        }));
        let step = Step {
            command: "dailyrep",
            params: vec![("da", "2026-07-01".to_owned())],
            timeout: Duration::from_secs(120),
        };
        let reply = execute(&mut session, &step, &mut Silent).expect("confirmed report");
        assert_eq!(reply.fields(), &[]);
    }
}

#[test]
fn a_late_foreign_error_cannot_impersonate_the_pending_report_refusal() {
    let mut session = Session::new(Box::new(LateReply {
        reads: VecDeque::new(),
        then: reply("!sdev\tds0\t"),
        answer: reply("scnt\t?382\tcmdailyrep\t"),
    }));
    let step = Step {
        command: "dailyrep",
        params: vec![("da", "2026-07-01".to_owned())],
        timeout: Duration::from_secs(120),
    };
    assert_eq!(
        execute(&mut session, &step, &mut Silent)
            .unwrap_err()
            .to_string(),
        "Oczekiwano odpowiedzi dailyrep, otrzymano scnt."
    );
}

#[derive(Default)]
struct StatusReplies(VecDeque<Vec<u8>>);

impl Port for StatusReplies {
    fn write_all(&mut self, bytes: &[u8], _timeout: Duration) -> io::Result<()> {
        let (command, fields) = frame::decode(bytes).map_err(io::Error::other)?;
        if matches!(command.as_str(), "!sprn" | "!sdev") {
            let token = fields.get("@").ok_or(io::ErrorKind::InvalidData)?;
            let status = if command == "!sprn" { "pr0" } else { "ds0" };
            self.0
                .push_back(reply(&format!("{command}\t@{token}\t{status}\t")));
            if command == "!sdev" {
                self.0.push_back(b"\x02noise\t#0000\x03".to_vec());
            }
        }
        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        let bytes = self.0.pop_front().ok_or(io::ErrorKind::TimedOut)?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

#[test]
fn two_ready_status_replies_end_the_watch_without_reading_unrelated_trailing_noise() {
    let mut session = Session::new(Box::<StatusReplies>::default());
    let step = Step {
        command: "dailyrep",
        params: vec![("da", "2026-07-01".to_owned())],
        timeout: Duration::from_secs(120),
    };
    assert!(matches!(
        execute(&mut session, &step, &mut Silent),
        Err(Error::Timeout("dailyrep"))
    ));
}
