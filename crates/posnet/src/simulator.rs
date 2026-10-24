//! Offline TCP Temo peer using the serial driver's [`Session`](crate::session::Session),
//! with one session at a time.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt::Write as _,
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

use jiff::{Timestamp, tz::TimeZone};

use crate::{
    Error, decimal,
    frame::{self, ETX, MAX_FRAME, Reply},
    printer::PRINTER_ZONE,
    receipt::Quantity,
    session::{Link, Port},
    text,
};

pub const SERIAL: &str = "DEMO0000001";
const POLL: Duration = Duration::from_millis(1);
const DEFAULT_VAT: [&str; 7] = [
    "23,00", "8,00", "0,00", "101,00", "101,00", "101,00", "100,00",
];

pub type Params = BTreeMap<String, String>;

/// Scripted command refusals, effects and reply failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Documented `ERR\t?2000\tcm<command>\tfd\t` refusal; no command effect.
    Error,
    /// Custom reply parameters; no command effect.
    Reply(Vec<(String, String)>),
    /// Execute successfully, then discard the reply.
    LoseReply,
    /// Execute successfully, then return a reply with a bad CRC.
    CorruptReply,
    /// Execute successfully, then close the connection.
    Disconnect,
    /// Execute successfully, then emit `<command>\t?2000\t` (not an `ERR` rejection).
    ExecutionError,
    /// Documented error form, optionally naming a different command.
    Device {
        code: u32,
        command: String,
        field: String,
    },
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

/// Polish civil time, independent of the host's zone.
#[derive(Debug)]
struct Clock(TimeZone);

impl Default for Clock {
    #[expect(
        clippy::expect_used,
        reason = "test infrastructure: no zone database means no printer clock"
    )]
    fn default() -> Self {
        Self(TimeZone::get(PRINTER_ZONE).expect("the printer's zone in the zone database"))
    }
}

#[derive(Debug, Default)]
struct State {
    header: String,
    vat: [String; 7],
    clock: Clock,
    timestamp: Option<Timestamp>,
    status: BTreeMap<String, String>,
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
    holding: Option<String>,
    paper_out: bool,
    out_of_paper_at: Option<String>,
}

/// Blocks a command until the test releases it.
#[derive(Debug)]
pub struct Hold {
    reached: Receiver<()>,
    release: Sender<()>,
}

impl Hold {
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
    connected: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Simulator {
    /// Panics if local sockets, threads or the zone database are unavailable.
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
            connected: Arc::new(AtomicBool::new(false)),
            thread: Some(thread),
        }
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn link(&self) -> Arc<dyn Link> {
        self.link_with(vec![self.address.clone()], Vec::new())
    }

    /// Offers `candidates` for detection. `denied` ports refuse permission;
    /// ports other than this simulator do not exist.
    pub fn link_with(&self, candidates: Vec<String>, denied: Vec<String>) -> Arc<dyn Link> {
        Arc::new(SimLink {
            address: self.address.clone(),
            candidates,
            denied,
            connected: Arc::clone(&self.connected),
            state: Arc::clone(&self.state),
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

    pub fn frames(&self) -> Vec<Vec<u8>> {
        self.state().frames.clone()
    }

    pub fn transaction_open(&self) -> bool {
        self.state().transaction_open
    }

    /// Simulates opening or closing a transaction at the keypad.
    pub fn set_transaction_open(&self, open: bool) {
        self.state().transaction_open = open;
    }

    pub fn set_header(&self, header: &str) {
        header.clone_into(&mut self.state().header);
    }

    /// `vatget` slots `va`–`vg`: e.g. `"23,00"`; `"101,00"` marks an unused slot.
    pub fn set_vat(&self, rates: [&str; 7]) {
        self.state().vat = rates.map(str::to_owned);
    }

    /// Freezes the printer clock, independently of host time and timezone.
    pub fn set_clock(&self, timestamp: Timestamp) {
        self.state().timestamp = Some(timestamp);
    }

    /// Overrides a status field in `sdev`, `sprn`, `scomm` or `strns`.
    pub fn set_status(&self, field: &str, value: &str) {
        drop(
            self.state()
                .status
                .insert(field.to_owned(), value.to_owned()),
        );
    }

    pub fn fault(&self, command: &str, fault: Fault) {
        drop(self.state().faults.insert(command.to_owned(), fault));
    }

    pub fn clear_fault(&self, command: &str) {
        drop(self.state().faults.remove(command));
    }

    /// Without paper, status queries still answer; printing commands wait without replying.
    pub fn set_paper(&self, present: bool) {
        self.state().paper_out = !present;
    }

    /// The next matching command waits for paper before executing.
    pub fn run_out_of_paper_at(&self, command: &str) {
        self.state().out_of_paper_at = Some(command.to_owned());
    }

    /// Holds the next matching command before execution until [`Hold::release`].
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
        // Wake accept so the thread sees the stop flag.
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
    connected: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
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
        if self.connected.swap(true, Ordering::SeqCst) {
            return Err(refuse(io::ErrorKind::PermissionDenied));
        }
        let stream = TcpStream::connect(port).map_err(|source| {
            self.connected.store(false, Ordering::SeqCst);
            Error::Open {
                port: port.to_owned(),
                source,
            }
        })?;
        stream.set_nodelay(true).map_err(Error::Io)?;
        Ok(Box::new(SimPort {
            inner: TcpPort(stream),
            connected: Arc::clone(&self.connected),
            state: Arc::clone(&self.state),
        }))
    }

    fn candidates(&self) -> Vec<String> {
        self.candidates.clone()
    }

    fn time_scale(&self) -> u32 {
        1000
    }
}

struct SimPort {
    inner: TcpPort,
    connected: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
}

impl Drop for SimPort {
    fn drop(&mut self) {
        self.connected.store(false, Ordering::SeqCst);
    }
}

impl Port for SimPort {
    fn write_all(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        self.inner.write_all(bytes, timeout)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.inner.read(buffer, timeout)
    }

    fn operator_wait_limit(&self, scaled: Duration) -> Duration {
        if lock(&self.state).holding.is_some() {
            Duration::from_secs(600)
        } else {
            scaled
        }
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
            lock(state).holding = None;
        }
    }
}

fn serve_session(mut stream: TcpStream, state: &Mutex<State>, stop: &AtomicBool) {
    if stream.set_read_timeout(Some(POLL)).is_err() || stream.set_nodelay(true).is_err() {
        return;
    }
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut waiting = VecDeque::<(Vec<u8>, Option<Held>)>::new();
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => buffer.extend_from_slice(chunk.get(..read).unwrap_or_default()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {},
            Err(_) => return,
        }
        if buffer.len() > MAX_FRAME {
            buffer.clear();
        }
        while let Some(end) = buffer.iter().position(|byte| *byte == ETX) {
            let received: Vec<u8> = buffer.drain(..=end).collect();
            lock(state).frames.push(received.clone());
            let command = frame::decode(&received)
                .ok()
                .map(|(command, _)| command)
                .unwrap_or_default();
            let held = lock(state).held.take_if(|held| held.command == command);
            if let Some(held) = &held {
                lock(state).holding = Some(command.clone());
                let _sent = held.reached.send(());
            }
            if !command.starts_with('!')
                && (held.is_some() || waits_for_paper(&command, state, !waiting.is_empty()))
            {
                waiting.push_back((received, held));
            } else if !deliver(&mut stream, answer(&received, state)) {
                return;
            }
        }
        while let Some((_, held)) = waiting.front_mut() {
            if let Some(gate) = held {
                match gate.release.try_recv() {
                    Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                        *held = None;
                        lock(state).holding = None;
                    },
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            if lock(state).paper_out {
                break;
            }
            let Some((received, _)) = waiting.pop_front() else {
                break;
            };
            if !deliver(&mut stream, answer(&received, state)) {
                return;
            }
        }
    }
}

/// Printing commands wait for paper; asynchronous queries bypass the queue (spec p. 15).
fn waits_for_paper(command: &str, state: &Mutex<State>, queued: bool) -> bool {
    if queued {
        return true;
    }
    let mut state = lock(state);
    if state.out_of_paper_at.as_deref() == Some(command) {
        state.out_of_paper_at = None;
        state.paper_out = true;
    }
    state.paper_out && state.query(command).is_none()
}

enum Answer {
    Reply(Vec<u8>),
    Silent,
    Disconnect,
}

fn deliver(stream: &mut TcpStream, answer: Answer) -> bool {
    match answer {
        Answer::Reply(bytes) => stream.write_all(&bytes).is_ok(),
        Answer::Silent => true,
        Answer::Disconnect => false,
    }
}

fn answer(received: &[u8], state: &Mutex<State>) -> Answer {
    let Ok((command, params)) = frame::decode(received) else {
        return Answer::Reply(encoded("ERR\t?2000\t"));
    };
    let mut state = lock(state);
    let fault = state.faults.get(&command).cloned();
    let outcome = match &fault {
        Some(Fault::Error | Fault::Device { .. }) => Err(()),
        Some(Fault::Reply(reply)) => Ok(reply.clone()),
        _ => state.handle(&command, &params),
    };
    let execution_error = fault == Some(Fault::ExecutionError);
    let mut payload = if outcome.is_err() && !execution_error {
        "ERR".to_owned()
    } else {
        command.clone()
    };
    payload.push('\t');
    if let Some(token) = params.get("@") {
        payload.push('@');
        payload.push_str(token);
        payload.push('\t');
    }
    if execution_error {
        payload.push_str("?2000\t");
    } else if let Ok(reply) = outcome {
        for (key, value) in reply {
            payload.push_str(&key);
            payload.push_str(&value);
            payload.push('\t');
        }
    } else {
        let (code, named, field) = match &fault {
            Some(Fault::Device {
                code,
                command,
                field,
            }) => (*code, command.as_str(), field.as_str()),
            _ => (2000, command.as_str(), ""),
        };
        let _written = write!(payload, "?{code:04}\tcm{named}\tfd{field}\t");
    }
    match fault {
        Some(Fault::LoseReply) => Answer::Silent,
        Some(Fault::Disconnect) => Answer::Disconnect,
        Some(Fault::CorruptReply) => {
            let mut bytes = encoded(&payload);
            if let Some(byte) = bytes.get_mut(1) {
                *byte ^= 1;
            }
            Answer::Reply(bytes)
        },
        _ => Answer::Reply(encoded(&payload)),
    }
}

/// Reply values allow line feeds (headers), unlike outgoing command values.
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

    fn query(&self, command: &str) -> Option<Vec<(String, String)>> {
        let mut fields = match command {
            "getrealid" => reply(&[
                ("nm", "POSNET TEMO ONLINE"),
                ("vr", "32.01"),
                ("nu", SERIAL),
            ]),
            // `!`-prefixed queries run asynchronously (spec pp. 384-385).
            "sdev" | "!sdev" => reply(&[
                ("ds", if self.holding.is_some() { "2" } else { "0" }),
                ("cp", "1"),
                ("qe", "1"),
                ("pe", "0"),
                ("tl", "0"),
            ]),
            "sprn" | "!sprn" => reply(&[("pr", if self.paper_out { "5" } else { "0" })]),
            "scomm" => reply(&[
                ("fs", "1"),
                ("tz", "0"),
                ("ts", if self.transaction_open { "16" } else { "0" }),
                ("hr", "1"),
                ("td", "0"),
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
            "rtcget" => {
                let time = self
                    .timestamp
                    .unwrap_or_else(Timestamp::now)
                    .to_zoned(self.clock.0.clone());
                vec![
                    ("da".to_owned(), time.strftime("%Y-%m-%d;%H:%M").to_string()),
                    (
                        "tm".to_owned(),
                        time.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
                    ),
                ]
            },
            _ => return None,
        };
        for (key, value) in &mut fields {
            if let Some(override_value) = self.status.get(key) {
                value.clone_from(override_value);
            }
        }
        Some(fields)
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

#[cfg(test)]
mod tests;
