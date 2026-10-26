//! Mixed-brand fixtures route only to offline peers, never `SerialLink`.
use elzab::{
    frame as elzab_frame,
    simulator::{Fault as ElzabFault, Simulator as ElzabSimulator},
};
use fiscal::{
    device::Vendor,
    error::Error,
    link::{Candidate, Link, Port},
};
use novitus::{
    frame as novitus_frame,
    simulator::{Fault as NovitusFault, Simulator as NovitusSimulator},
};
use posnet::simulator::{Fault as PosnetFault, Simulator as PosnetSimulator};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(crate) const VENDORS: [Vendor; 3] = [Vendor::Posnet, Vendor::Elzab, Vendor::Novitus];

pub(crate) enum Simulator {
    Posnet(PosnetSimulator),
    Elzab(ElzabSimulator),
    Novitus(NovitusSimulator),
}
impl Simulator {
    pub(crate) fn start(vendor: Vendor) -> Self {
        match vendor {
            Vendor::Posnet => Self::Posnet(PosnetSimulator::start()),
            Vendor::Elzab => Self::Elzab(ElzabSimulator::start()),
            Vendor::Novitus => {
                let sim = NovitusSimulator::start();
                sim.set_identity("NOVITUS HD II ONLINE", "3.50");
                Self::Novitus(sim)
            },
        }
    }

    pub(crate) fn address(&self) -> &str {
        match self {
            Self::Posnet(sim) => sim.address(),
            Self::Elzab(sim) => sim.address(),
            Self::Novitus(sim) => sim.address(),
        }
    }

    pub(crate) fn link(&self) -> Arc<dyn Link> {
        match self {
            Self::Posnet(sim) => sim.link(),
            Self::Elzab(sim) => sim.link(),
            Self::Novitus(sim) => sim.link(),
        }
    }

    pub(crate) fn frames(&self) -> Vec<Vec<u8>> {
        match self {
            Self::Posnet(sim) => sim.frames(),
            Self::Elzab(sim) => sim.frames(),
            Self::Novitus(sim) => sim.frames(),
        }
    }

    pub(crate) fn receipts(&self) -> usize {
        match self {
            Self::Posnet(sim) => sim.receipts().len(),
            Self::Elzab(sim) => usize::try_from(sim.receipts()).unwrap(),
            Self::Novitus(sim) => usize::try_from(sim.receipts()).unwrap(),
        }
    }

    pub(crate) fn reports(&self) -> usize {
        match self {
            Self::Posnet(sim) => sim.reports().len(),
            Self::Elzab(sim) => usize::try_from(sim.reports()).unwrap(),
            Self::Novitus(sim) => usize::try_from(sim.reports()).unwrap(),
        }
    }

    pub(crate) fn prepare_receipt(&self) {
        match self {
            Self::Posnet(sim) => {
                sim.set_header("Warsztat Kowalski");
                sim.set_clock("2026-10-16T10:34:00Z".parse().unwrap());
            },
            Self::Elzab(sim) => {
                sim.set_header("Warsztat Kowalski");
                sim.set_clock("261016123400");
            },
            Self::Novitus(sim) => {
                sim.set_header("Warsztat Kowalski");
                sim.set_clock("26;10;16;12;34;0");
            },
        }
    }

    pub(crate) fn lose_final_reply(&self) {
        match self {
            Self::Posnet(sim) => sim.fault("trend", PosnetFault::LoseReply),
            Self::Elzab(sim) => sim.fault("LCL", ElzabFault::LoseReply),
            Self::Novitus(sim) => sim.fault("28;0$z", NovitusFault::LoseReply),
        }
    }

    pub(crate) fn disconnect_final_reply(&self) {
        match self {
            Self::Posnet(sim) => sim.fault("trend", PosnetFault::Disconnect),
            Self::Elzab(sim) => sim.fault("LCL", ElzabFault::Disconnect),
            Self::Novitus(sim) => sim.fault("28;0$z", NovitusFault::Disconnect),
        }
    }

    pub(crate) fn fiscal_frames(&self) -> Vec<Vec<u8>> {
        self.frames()
            .into_iter()
            .filter(|frame| match self {
                Self::Posnet(_) => frame.get(1..).is_some_and(|body| body.starts_with(b"tr")),
                Self::Elzab(_) => elzab_frame::decode(frame).is_ok_and(|(_, body)| {
                    [
                        b"LOR".as_slice(),
                        b"LIT",
                        b"LDS",
                        b"LBP",
                        b"LCL",
                        b"LNP",
                        b"LAL",
                        b"RRP",
                        b"RPP",
                    ]
                    .iter()
                    .any(|command| body.starts_with(command))
                }),
                Self::Novitus(_) => novitus_frame::unframe(frame).is_ok_and(|body| {
                    [
                        b"$h".as_slice(),
                        b"$l",
                        b"$b",
                        b"$y",
                        b"$z",
                        b"@z",
                        b"#r",
                        b"#o",
                    ]
                    .iter()
                    .any(|command| body.windows(command.len()).any(|part| part == *command))
                }),
            })
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct RoutingLink {
    routes: Vec<(Candidate, Arc<dyn Link>)>,
    pub(crate) opened: Mutex<Vec<String>>,
    pub(crate) enumerations: AtomicUsize,
}
impl RoutingLink {
    pub(crate) fn new(routes: Vec<(Candidate, Arc<dyn Link>)>) -> Arc<Self> {
        Arc::new(Self {
            routes,
            opened: Mutex::new(Vec::new()),
            enumerations: AtomicUsize::new(0),
        })
    }

    pub(crate) fn enumeration_count(&self) -> usize {
        self.enumerations.load(Ordering::SeqCst)
    }
}
impl Link for RoutingLink {
    #[expect(
        clippy::unwrap_in_result,
        reason = "a poisoned offline routing fixture must fail its test"
    )]
    fn open(&self, port: &str, baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        self.opened.lock().unwrap().push(port.to_owned());
        self.routes
            .iter()
            .find(|(candidate, _)| candidate.port == port)
            .map_or_else(
                || {
                    Err(Error::Open {
                        port: port.to_owned(),
                        source: io::ErrorKind::NotFound.into(),
                    })
                },
                |(_, link)| link.open(port, baud_rate),
            )
    }

    fn candidates(&self) -> Vec<Candidate> {
        let _previous = self.enumerations.fetch_add(1, Ordering::SeqCst);
        self.routes
            .iter()
            .map(|(candidate, _)| candidate.clone())
            .collect()
    }

    fn time_scale(&self) -> u32 {
        1000
    }
}
