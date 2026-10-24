//! An offline Temo Online peer. Commands travel over a real local TCP socket through the same
//! [`Session`](crate::session::Session) code as a serial port, one session at a time.

use std::{
    collections::{BTreeMap, HashMap},
    io::{self, Read as _, Write as _},
    mem,
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    Error, decimal,
    frame::{self, ETX, MAX_FRAME, Reply},
    receipt::Quantity,
    session::{Link, Port},
    text,
};

pub const SERIAL: &str = "DEMO0000001";
const POLL: Duration = Duration::from_millis(50);
const DEFAULT_VAT: [&str; 7] = [
    "23,00", "8,00", "0,00", "101,00", "101,00", "101,00", "100,00",
];

pub type Params = BTreeMap<String, String>;

/// How the simulated device answers a command instead of executing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The documented execution error form `<command>\t?2000\t`.
    Error,
    /// These parameters instead of the real answer; the command has no effect.
    Reply(Vec<(String, String)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimReceipt {
    pub total: u64,
    pub lines: Vec<Params>,
    pub payments: Vec<Params>,
    pub buyer_nip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimReport {
    pub command: String,
    pub params: Params,
}

#[derive(Debug)]
struct Held {
    command: String,
    reached: Sender<()>,
    release: Receiver<()>,
}

#[derive(Debug, Default)]
struct State {
    header: String,
    vat: [String; 7],
    transaction_open: bool,
    footer_open: bool,
    lines: Vec<Params>,
    advances: Vec<Params>,
    payments: Vec<Params>,
    buyer_nip: Option<String>,
    receipts: Vec<SimReceipt>,
    reports: Vec<SimReport>,
    footers: Vec<Params>,
    frames: Vec<Vec<u8>>,
    faults: HashMap<String, Fault>,
    held: Option<Held>,
}

/// A command the simulator holds until the test releases it.
#[derive(Debug)]
pub struct Hold {
    reached: Receiver<()>,
    release: Sender<()>,
}

impl Hold {
    /// Whether the held command arrived within `timeout`.
    pub fn reached(&self, timeout: Duration) -> bool {
        self.reached.recv_timeout(timeout).is_ok()
    }

    pub fn release(&self) {
        let _sent = self.release.send(());
    }
}

#[derive(Debug)]
pub struct Simulator {
    address: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Simulator {
    /// # Panics
    /// When no local socket or thread is available: a broken test environment.
    #[expect(
        clippy::expect_used,
        reason = "test infrastructure: no socket means no test"
    )]
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the simulator socket");
        let address = listener
            .local_addr()
            .expect("simulator address")
            .to_string();
        let state = Arc::new(Mutex::new(State {
            header: "&c&1Warsztat Kowalski&1&c\nul. Testowa 1\n00-001 Warszawa".to_owned(),
            vat: DEFAULT_VAT.map(str::to_owned),
            ..State::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("Temo simulator".to_owned())
            .spawn({
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                move || serve(&listener, &state, &stop)
            })
            .expect("start the simulator thread");
        Self {
            address,
            state,
            stop,
            thread: Some(thread),
        }
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    /// A link whose only port is this simulator.
    pub fn link(&self) -> Arc<dyn Link> {
        self.link_with(vec![self.address.clone()], Vec::new())
    }

    /// A link offering `candidates` during detection; ports in `denied` refuse to open as a port
    /// without permission does. Any port other than the simulator's own does not exist.
    pub fn link_with(&self, candidates: Vec<String>, denied: Vec<String>) -> Arc<dyn Link> {
        Arc::new(SimLink {
            address: self.address.clone(),
            candidates,
            denied,
        })
    }

    pub fn receipts(&self) -> Vec<SimReceipt> {
        self.state().receipts.clone()
    }

    pub fn reports(&self) -> Vec<SimReport> {
        self.state().reports.clone()
    }

    pub fn footers(&self) -> Vec<Params> {
        self.state().footers.clone()
    }

    /// Every command frame received, in order.
    pub fn frames(&self) -> Vec<Vec<u8>> {
        self.state().frames.clone()
    }

    pub fn transaction_open(&self) -> bool {
        self.state().transaction_open
    }

    /// Opens or closes the transaction as an operator at the keypad would.
    pub fn set_transaction_open(&self, open: bool) {
        self.state().transaction_open = open;
    }

    pub fn set_header(&self, header: &str) {
        header.clone_into(&mut self.state().header);
    }

    /// The `vatget` table, `va` to `vg`, e.g. `"23,00"`; `"101,00"` is an unused slot.
    pub fn set_vat(&self, rates: [&str; 7]) {
        self.state().vat = rates.map(str::to_owned);
    }

    pub fn fault(&self, command: &str, fault: Fault) {
        drop(self.state().faults.insert(command.to_owned(), fault));
    }

    pub fn clear_fault(&self, command: &str) {
        drop(self.state().faults.remove(command));
    }

    /// Holds the next `command` before executing it until [`Hold::release`].
    pub fn hold(&self, command: &str) -> Hold {
        let (reached, reached_receiver) = mpsc::channel();
        let (release_sender, release) = mpsc::channel();
        self.state().held = Some(Held {
            command: command.to_owned(),
            reached,
            release,
        });
        Hold {
            reached: reached_receiver,
            release: release_sender,
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the blocking accept so the thread sees the stop flag.
        drop(TcpStream::connect(&self.address));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

#[derive(Debug)]
struct SimLink {
    address: String,
    candidates: Vec<String>,
    denied: Vec<String>,
}

impl Link for SimLink {
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        let refuse = |kind: io::ErrorKind| Error::Open {
            port: port.to_owned(),
            source: kind.into(),
        };
        if self.denied.iter().any(|denied| denied == port) {
            return Err(refuse(io::ErrorKind::PermissionDenied));
        }
        if port != self.address {
            return Err(refuse(io::ErrorKind::NotFound));
        }
        let stream = TcpStream::connect(port).map_err(|source| Error::Open {
            port: port.to_owned(),
            source,
        })?;
        Ok(Box::new(TcpPort(stream)))
    }

    fn candidates(&self) -> Vec<String> {
        self.candidates.clone()
    }
}

pub(crate) struct TcpPort(pub(crate) TcpStream);

impl Port for TcpPort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.0
            .set_write_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        self.0.write_all(bytes)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.0
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        match self.0.read(buffer) {
            Ok(0) => Err(io::ErrorKind::UnexpectedEof.into()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Err(io::ErrorKind::TimedOut.into())
            },
            result => result,
        }
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn serve(listener: &TcpListener, state: &Mutex<State>, stop: &AtomicBool) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(stream) = stream {
            serve_session(stream, state, stop);
        }
    }
}

/// One session at a time, like a serial port: each driver session is a new connection.
fn serve_session(mut stream: TcpStream, state: &Mutex<State>, stop: &AtomicBool) {
    if stream.set_read_timeout(Some(POLL)).is_err() {
        return;
    }
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => buffer.extend_from_slice(chunk.get(..read).unwrap_or_default()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            },
            Err(_) => return,
        }
        if buffer.len() > MAX_FRAME {
            buffer.clear();
        }
        while let Some(end) = buffer.iter().position(|byte| *byte == ETX) {
            let received: Vec<u8> = buffer.drain(..=end).collect();
            if stream.write_all(&answer(&received, state)).is_err() {
                return;
            }
        }
    }
}

fn answer(received: &[u8], state: &Mutex<State>) -> Vec<u8> {
    lock(state).frames.push(received.to_vec());
    let Ok((command, params)) = frame::decode(received) else {
        return encoded("ERR\t?2000\t");
    };
    let held = lock(state).held.take_if(|held| held.command == command);
    if let Some(held) = held {
        let _sent = held.reached.send(());
        let _released = held.release.recv();
    }
    let mut state = lock(state);
    let outcome = match state.faults.get(&command).cloned() {
        Some(Fault::Error) => Err(()),
        Some(Fault::Reply(reply)) => Ok(reply),
        None => state.handle(&command, &params),
    };
    match outcome {
        Ok(reply) => {
            let mut payload = command;
            payload.push('\t');
            for (key, value) in reply {
                payload.push_str(&key);
                payload.push_str(&value);
                payload.push('\t');
            }
            encoded(&payload)
        },
        Err(()) => encoded(&format!("{command}\t?2000\t")),
    }
}

/// Device replies may carry a line feed inside a value (the header), which the driver's outgoing
/// check rightly rejects, so replies are framed here directly.
fn encoded(payload: &str) -> Vec<u8> {
    frame::wrap(&text::encode(payload).unwrap_or_default())
}

fn reply(fields: &[(&str, &str)]) -> Vec<(String, String)> {
    fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn params(reply: &Reply) -> Params {
    reply.fields().iter().cloned().collect()
}

fn number(params: &Params, key: &str) -> Result<u64, ()> {
    params
        .get(key)
        .and_then(|value| value.parse().ok())
        .ok_or(())
}

impl State {
    fn active_slots(&self) -> Vec<String> {
        self.vat
            .iter()
            .enumerate()
            .filter(|(_, rate)| {
                decimal::parse(&rate.replace(',', "."), 2).is_some_and(|units| units <= 10_000)
            })
            .map(|(slot, _)| slot.to_string())
            .collect()
    }

    /// Read-only status queries; `None` for anything else.
    fn query(&self, command: &str) -> Option<Vec<(String, String)>> {
        Some(match command {
            "getrealid" => reply(&[
                ("nm", "POSNET TEMO ONLINE"),
                ("vr", "32.01"),
                ("nu", SERIAL),
            ]),
            "sdev" => reply(&[("ds", "0"), ("cp", "1"), ("qe", "1"), ("pe", "0")]),
            "sprn" => reply(&[("pr", "0")]),
            "scomm" => reply(&[
                ("fs", "1"),
                ("tz", "0"),
                ("ts", if self.transaction_open { "16" } else { "0" }),
                ("hr", "1"),
                ("nu", SERIAL),
            ]),
            "strns" => reply(&[
                ("to", if self.transaction_open { "1" } else { "0" }),
                ("ts", "16"),
                ("fe", if self.footer_open { "1" } else { "0" }),
            ]),
            "hdrget" => vec![("tx".to_owned(), self.header.clone())],
            "scnt" => vec![("bt".to_owned(), self.receipts.len().to_string())],
            "vatget" => ["va", "vb", "vc", "vd", "ve", "vf", "vg"]
                .iter()
                .zip(&self.vat)
                .map(|(key, rate)| ((*key).to_owned(), rate.clone()))
                .collect(),
            "rtcget" => vec![(
                "da".to_owned(),
                jiff::Zoned::now().strftime("%Y-%m-%d;%H:%M").to_string(),
            )],
            _ => return None,
        })
    }

    fn handle(&mut self, command: &str, reply_params: &Reply) -> Result<Vec<(String, String)>, ()> {
        if let Some(answer) = self.query(command) {
            return Ok(answer);
        }
        let params = params(reply_params);
        match command {
            "trinit" if !self.transaction_open => {
                self.transaction_open = true;
                self.lines.clear();
                self.advances.clear();
                self.payments.clear();
                self.buyer_nip = None;
                Ok(Vec::new())
            },
            "trnipset" if self.transaction_open => {
                let nip = params.get("ni").ok_or(())?;
                if nip.trim().is_empty() || nip.chars().count() > text::MAX_BUYER_NIP_LENGTH {
                    return Err(());
                }
                self.buyer_nip = Some(nip.clone());
                Ok(Vec::new())
            },
            "trline" if self.transaction_open => {
                let quantity = params
                    .get("il")
                    .and_then(|value| Quantity::parse(value).ok())
                    .ok_or(())?;
                let value = quantity.times(number(&params, "pr")?).ok_or(())?;
                let slot = params.get("vt").ok_or(())?;
                if number(&params, "wa")? != value || !self.active_slots().contains(slot) {
                    return Err(());
                }
                self.lines.push(params);
                Ok(Vec::new())
            },
            "tradvance" if self.transaction_open => {
                let slot = params.get("vt").ok_or(())?;
                let sales: u64 = self
                    .lines
                    .iter()
                    .filter(|line| line.get("vt") == Some(slot))
                    .map(|line| number(line, "wa"))
                    .sum::<Result<u64, ()>>()?;
                let amount = number(&params, "pr")?;
                if !self.lines.iter().any(|line| line.get("vt") == Some(slot))
                    || number(&params, "wa")? != sales
                    || amount > sales
                    || self
                        .advances
                        .iter()
                        .any(|advance| advance.get("vt") == Some(slot))
                {
                    return Err(());
                }
                self.advances.push(params);
                Ok(Vec::new())
            },
            "trpayment" if self.transaction_open => {
                let kind = params.get("ty").map(String::as_str).ok_or(())?;
                let named = params.get("na").map(String::as_str);
                if !["0", "2", "5", "6", "8"].contains(&kind)
                    || (kind == "6" && named != Some("Płatność mobilna"))
                {
                    return Err(());
                }
                let _: u64 = number(&params, "wa")?;
                self.payments.push(params);
                Ok(Vec::new())
            },
            "trend" if self.transaction_open && !self.lines.is_empty() => {
                self.end_transaction(&params)
            },
            "trftrln" if self.footer_open => {
                self.footers.push(params);
                Ok(Vec::new())
            },
            "trftrend" if self.footer_open => {
                self.footer_open = false;
                Ok(Vec::new())
            },
            "dailyrep" | "monthlyrep" | "periodicrepbydates" if !self.transaction_open => {
                self.reports.push(SimReport {
                    command: command.to_owned(),
                    params,
                });
                Ok(Vec::new())
            },
            _ => Err(()),
        }
    }

    fn end_transaction(&mut self, params: &Params) -> Result<Vec<(String, String)>, ()> {
        let sales = self
            .lines
            .iter()
            .map(|line| number(line, "wa"))
            .sum::<Result<u64, ()>>()?;
        let advances = self
            .advances
            .iter()
            .map(|advance| number(advance, "pr"))
            .sum::<Result<u64, ()>>()?;
        let paid = self
            .payments
            .iter()
            .map(|payment| number(payment, "wa"))
            .sum::<Result<u64, ()>>()?;
        let total = sales.checked_sub(advances).ok_or(())?;
        if total != number(params, "to")? || total != paid || paid != number(params, "fp")? {
            return Err(());
        }
        self.receipts.push(SimReceipt {
            total,
            lines: mem::take(&mut self.lines),
            payments: mem::take(&mut self.payments),
            buyer_nip: self.buyer_nip.take(),
        });
        self.transaction_open = false;
        self.footer_open = params.get("fe").is_some_and(|value| value == "0");
        Ok(Vec::new())
    }
}
