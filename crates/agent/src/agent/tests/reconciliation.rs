use proptest::{prelude::*, sample, test_runner::Config};
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use tempfile::TempDir;

use super::*;

#[derive(Clone, Copy, Debug)]
enum Evidence {
    NoDispatch,
    Dispatching,
    Printed,
    Unnumbered,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Remote {
    Taken,
    Unknown,
    Done,
    Failed,
    Expired,
}
impl Remote {
    const fn label(self) -> &'static str {
        match self {
            Self::Taken => "taken",
            Self::Unknown => "unknown",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Expired => "expired",
        }
    }
}
#[derive(Clone, Debug)]
struct Model {
    evidence: Evidence,
    remote: Remote,
    settled: bool,
}
#[derive(Clone, Debug)]
enum Transition {
    Settle,
    NetworkFailure,
    StaffDone,
    StaffFailed,
    Acknowledge,
}
#[derive(Debug)]
struct Reconciliation;
impl ReferenceStateMachine for Reconciliation {
    type State = Model;
    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Model> {
        (0u8..4, 0u8..5)
            .prop_map(|(evidence, remote)| Model {
                evidence: match evidence {
                    0 => Evidence::NoDispatch,
                    1 => Evidence::Dispatching,
                    2 => Evidence::Printed,
                    _ => Evidence::Unnumbered,
                },
                remote: match remote {
                    0 => Remote::Taken,
                    1 => Remote::Unknown,
                    2 => Remote::Done,
                    3 => Remote::Failed,
                    _ => Remote::Expired,
                },
                settled: false,
            })
            .boxed()
    }

    fn transitions(state: &Model) -> BoxedStrategy<Transition> {
        let mut transitions = vec![Transition::Settle, Transition::NetworkFailure];
        if state.remote == Remote::Unknown {
            transitions.extend([Transition::StaffDone, Transition::StaffFailed]);
        }
        if matches!(state.evidence, Evidence::Printed | Evidence::Unnumbered)
            && matches!(state.remote, Remote::Failed | Remote::Expired)
            && !state.settled
        {
            transitions.push(Transition::Acknowledge);
        }
        sample::select(transitions).boxed()
    }

    fn apply(mut state: Model, transition: &Transition) -> Model {
        match transition {
            Transition::StaffDone => state.remote = Remote::Done,
            Transition::StaffFailed => state.remote = Remote::Failed,
            Transition::Acknowledge => state.settled = true,
            Transition::NetworkFailure => {},
            Transition::Settle if state.settled => {},
            Transition::Settle => match state.remote {
                Remote::Done => state.settled = true,
                Remote::Failed | Remote::Expired => {
                    state.settled =
                        !matches!(state.evidence, Evidence::Printed | Evidence::Unnumbered);
                },
                Remote::Taken => match state.evidence {
                    Evidence::NoDispatch => {
                        state.remote = Remote::Failed;
                        state.settled = true;
                    },
                    Evidence::Printed => {
                        state.remote = Remote::Done;
                        state.settled = true;
                    },
                    Evidence::Dispatching | Evidence::Unnumbered => state.remote = Remote::Unknown,
                },
                Remote::Unknown => {
                    if matches!(state.evidence, Evidence::Printed) {
                        state.remote = Remote::Done;
                        state.settled = true;
                    }
                },
            },
        }
        state
    }
}
struct System {
    dir: StateDir,
    _temporary: TempDir,
    sim: Simulator,
    server: Holuto,
}
impl StateMachineTest for System {
    type Reference = Reconciliation;
    type SystemUnderTest = Self;

    fn init_test(ref_state: &Model) -> Self {
        let (temporary, dir) = state_dir();
        let sim = Simulator::start();
        let server = Holuto::new();
        server.insert(with(job_json(), &json!({"state":ref_state.remote.label()})));
        let device = match ref_state.evidence {
            Evidence::NoDispatch => Device::NotStarted,
            Evidence::Dispatching => Device::Dispatching {
                stage: "trend".to_owned(),
            },
            Evidence::Printed => Device::Completed {
                receipt_number: Some("41".to_owned()),
            },
            Evidence::Unnumbered => Device::Completed {
                receipt_number: None,
            },
        };
        Journal::new(&dir)
            .save(&Operation::Receipt(claimed(device, None).unwrap()))
            .unwrap();
        Self {
            dir,
            _temporary: temporary,
            sim,
            server,
        }
    }

    fn apply(state: Self, _model: &Model, transition: Transition) -> Self {
        match transition {
            Transition::StaffDone => state.server.resolve(JOB_ID, Some("41")).unwrap(),
            Transition::StaffFailed => state.server.resolve(JOB_ID, None).unwrap(),
            Transition::Settle => {
                let mut agent = agent(&state.sim, &state.dir, &state.server).unwrap();
                let _blocked = agent.settle().unwrap();
            },
            Transition::NetworkFailure => {
                state.server.fault(
                    "GET",
                    &format!("/agent/jobs/{JOB_ID}"),
                    Fault::Problem {
                        status: 503,
                        code: "unavailable".to_owned(),
                        detail: "temporary".to_owned(),
                    },
                );
                let mut agent = agent(&state.sim, &state.dir, &state.server).unwrap();
                let _result = agent.settle();
                // A settled journal emits no request, so consume an unused scripted failure.
                state.server.clear_faults();
            },
            Transition::Acknowledge => {
                let mut agent = agent(&state.sim, &state.dir, &state.server).unwrap();
                agent.acknowledge_orphaned_receipt().unwrap();
            },
        }
        state
    }

    fn check_invariants(state: &Self, ref_state: &Model) {
        assert_eq!(
            state.server.job(JOB_ID)["state"],
            ref_state.remote.label(),
            "server reconciliation agrees with reference after each step"
        );
        let operation = stored(&state.dir).unwrap();
        assert_eq!(
            operation.settled.is_some(),
            ref_state.settled,
            "journal settlement agrees with reference after each step"
        );
        assert_eq!(
            operation.attempt, ATTEMPT,
            "reconciliation never changes attempt"
        );
        assert_eq!(
            operation.issued_at.to_string(),
            "2025-01-02T12:00:00Z",
            "reconciliation never changes printer issue time"
        );
        assert!(
            state.sim.receipts().is_empty() && state.sim.reports().is_empty(),
            "every model transition is side-effect-free on the fiscal device"
        );
    }
}
prop_state_machine! {
    #![proptest_config(Config {cases:32,..Config::default()})]
    #[test]
    fn reconciliation_never_replays_fiscal_effects(sequential 1..12 => System);
}
