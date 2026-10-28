//! Offline TCP Temo peer using the driver's [`Session`](crate::session::Session), one at a time.
//! Receipt lines require canonical VAT-slot names (`0`–`6`) and an active rate.

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

use fiscal::{
    cp1250, decimal,
    device::{Capabilities, Device, Vendor},
    driver::Driver,
    error::Error,
    journal::{Journal, Printed},
    link::{Candidate, Connection, Link, Port},
    receipt::{Quantity, Receipt},
    report::Report,
};

use crate::{
    capabilities::MAX_BUYER_NIP,
    frame::{self, ETX, MAX_FRAME, Reply},
    printer::{PRINTER_ZONE, Printer},
};

pub const UNIQUE_NUMBER: &str = "DEM0000000001";
const POLL: Duration = Duration::from_millis(1);
const DEFAULT_VAT: [&str; 7] = [
    "23,00", "8,00", "5,00", "0,00", "100,00", "101,00", "101,00",
];

pub type Params = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Documented `ERR\t?2000\tcm<command>\tfd\t` refusal; no command effect.
    Error,
    /// Custom reply parameters; no command effect.
    Reply(Vec<(String, String)>),
    /// Executes, then drops the reply.
    LoseReply,
    /// Executes, then replies with a bad CRC.
    CorruptReply,
    /// Executes, then closes the connection.
    Disconnect,
    /// Executes, then emits `<command>\t?2000\t`, not an `ERR` rejection.
    ExecutionError,
    /// Emits the command-named `<command>\t?<code>\t` without executing (spec p. 13).
    Refused(u32),
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

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent simulated device flags, not states of one machine"
)]
#[derive(Debug, Default)]
struct State {
    header: String,
    capabilities: Option<Capabilities>,
    vat: [String; 7],
    clock: Clock,
    timestamp: Option<Timestamp>,
    fiscalised: Timestamp,
    status: BTreeMap<String, String>,
    transaction_open: bool,
    footer_open: bool,
    lines: Vec<Params>,
    advances: Vec<Params>,
    payments: Vec<Params>,
    buyer_nip: Option<String>,
    receipts: Vec<SimReceipt>,
    reports: Vec<SimReport>,
    frames: Vec<Vec<u8>>,
    faults: HashMap<String, Fault>,
    held: Option<Held>,
    holding: bool,
    paper_out: bool,
    /// `pccfgget` `cp` per `pi` interface number; WIN1250 (`0`) otherwise.
    code_pages: BTreeMap<String, String>,
}

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

    pub fn driver(&self) -> Box<dyn Driver> {
        Box::new(SimDriver {
            printer: Printer::new(
                self.link(),
                Connection {
                    port: self.address.clone(),
                    baud_rate: 9600,
                },
            ),
            state: Arc::clone(&self.state),
        })
    }

    pub fn set_capabilities(&self, capabilities: Capabilities) {
        self.state().capabilities = Some(capabilities);
    }

    /// Offers this simulator as the only detection candidate.
    pub fn link(&self) -> Arc<dyn Link> {
        Arc::new(SimLink {
            address: self.address.clone(),
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

    pub fn frames(&self) -> Vec<Vec<u8>> {
        self.state().frames.clone()
    }

    pub fn transaction_open(&self) -> bool {
        self.state().transaction_open
    }

    pub fn set_header(&self, header: &str) {
        header.clone_into(&mut self.state().header);
    }

    /// `vatget` slots `va`–`vg`: e.g. `"23,00"`; `"101,00"` marks an unused slot.
    pub fn set_vat(&self, rates: [&str; 7]) {
        self.state().vat = rates.map(str::to_owned);
    }

    /// Freezes printer time independently of the host clock.
    pub fn set_clock(&self, timestamp: Timestamp) {
        self.state().timestamp = Some(timestamp);
    }

    /// Sets the `fmrecfisc` fiscalisation time; the Unix epoch by default.
    pub fn set_fiscalised(&self, timestamp: Timestamp) {
        self.state().fiscalised = timestamp;
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

    /// Without paper, status queries still answer; printing commands wait without replying.
    pub fn set_paper(&self, present: bool) {
        self.state().paper_out = !present;
    }

    /// `pccfgget` `cp` for PC interface `"1"` or `"2"`.
    pub fn set_code_page(&self, interface: &str, code_page: &str) {
        drop(
            self.state()
                .code_pages
                .insert(interface.to_owned(), code_page.to_owned()),
        );
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

#[derive(Debug)]
struct SimDriver {
    printer: Printer,
    state: Arc<Mutex<State>>,
}

impl SimDriver {
    fn describe(&self, mut device: Device) -> Device {
        if let Some(capabilities) = &lock(&self.state).capabilities {
            device.capabilities = capabilities.clone();
        }
        device
    }
}

impl Driver for SimDriver {
    fn vendor(&self) -> Vendor {
        self.printer.vendor()
    }

    fn connection(&self) -> &Connection {
        self.printer.connection()
    }

    fn probe(&self) -> Result<Device, Error> {
        self.printer.probe().map(|device| self.describe(device))
    }

    fn print_receipt(
        &self,
        receipt: &Receipt,
        expected: &Device,
        journal: &mut dyn Journal,
    ) -> Result<Printed, Error> {
        self.printer.print_receipt(receipt, expected, journal)
    }

    fn print_report(
        &self,
        report: Report,
        unique_number: &str,
        journal: &mut dyn Journal,
    ) -> Result<(), Error> {
        self.printer.print_report(report, unique_number, journal)
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
    connected: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
}

impl Link for SimLink {
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        let refuse = |kind: io::ErrorKind| Error::Open {
            port: port.to_owned(),
            source: kind.into(),
        };
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
            stream,
            connected: Arc::clone(&self.connected),
            state: Arc::clone(&self.state),
        }))
    }

    fn candidates(&self) -> Vec<Candidate> {
        vec![Candidate {
            port: self.address.clone(),
            hint: None,
        }]
    }

    fn time_scale(&self) -> u32 {
        1000
    }
}

struct SimPort {
    stream: TcpStream,
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
        self.stream
            .set_write_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        self.stream.write_all(bytes)
    }

    fn read(&mut self, buffer: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.stream
            .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
        match self.stream.read(buffer) {
            Ok(0) => Err(io::ErrorKind::UnexpectedEof.into()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                Err(io::ErrorKind::TimedOut.into())
            },
            result => result,
        }
    }

    fn operator_wait_limit(&self, scaled: Duration) -> Duration {
        if lock(&self.state).holding {
            Duration::from_secs(600)
        } else {
            scaled
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
            lock(state).holding = false;
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
                lock(state).holding = true;
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
                        lock(state).holding = false;
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
    let state = lock(state);
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
        Some(Fault::Error | Fault::Device { .. } | Fault::Refused(_)) => Err(()),
        Some(Fault::Reply(reply)) => Ok(reply.clone()),
        _ => state.handle(&command, &params),
    };
    let command_error = match &fault {
        Some(Fault::ExecutionError) => Some(2000),
        Some(Fault::Refused(code)) => Some(*code),
        _ => None,
    };
    let mut payload = if outcome.is_err() && command_error.is_none() {
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
    if let Some(code) = command_error {
        let _written = write!(payload, "?{code:04}\t");
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
    frame::wrap(&cp1250::encode(payload).unwrap_or_default())
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
    fn active_slot(&self, slot: &str) -> bool {
        ["0", "1", "2", "3", "4", "5", "6"]
            .into_iter()
            .zip(&self.vat)
            .any(|(name, rate)| {
                name == slot
                    && decimal::parse(&rate.replace(',', "."), 2)
                        .is_some_and(|units| units <= 10_000)
            })
    }

    fn query(&self, command: &str) -> Option<Vec<(String, String)>> {
        let mut fields = match command {
            "getrealid" => reply(&[
                ("nm", "POSNET TEMO ONLINE"),
                ("vr", "32.01"),
                ("nu", UNIQUE_NUMBER),
            ]),
            // `!`-prefixed queries run asynchronously (spec pp. 384-385).
            "sdev" | "!sdev" => reply(&[
                ("ds", if self.holding { "2" } else { "0" }),
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
                ("nu", UNIQUE_NUMBER),
            ]),
            "strns" => reply(&[
                ("to", if self.transaction_open { "1" } else { "0" }),
                ("ts", "16"),
                ("fe", if self.footer_open { "1" } else { "0" }),
            ]),
            "hdrget" => vec![("tx".to_owned(), self.header.clone())],
            // Both PC interfaces: USB, Posnet protocol, WIN1250 (p. 155).
            "pccfgget" => reply(&[("id", "2"), ("no", "1"), ("pr", "0"), ("cp", "0")]),
            "scnt" => vec![("bt".to_owned(), self.receipts.len().to_string())],
            "vatget" => ["va", "vb", "vc", "vd", "ve", "vf", "vg"]
                .iter()
                .zip(&self.vat)
                .map(|(key, rate)| ((*key).to_owned(), rate.clone()))
                .collect(),
            "rtcget" => self.date_time(self.timestamp.unwrap_or_else(Timestamp::now)),
            // A non-fiscal (demo) printer has no fiscalisation record (p. 405).
            "fmrecfisc" if self.status.get("fs").is_none_or(|fs| fs != "0") => {
                self.date_time(self.fiscalised)
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

    /// `da` civil printer time and ISO8601 `tm` with offset (p. 17).
    fn date_time(&self, timestamp: Timestamp) -> Vec<(String, String)> {
        let time = timestamp.to_zoned(self.clock.0.clone());
        vec![
            ("da".to_owned(), time.strftime("%Y-%m-%d;%H:%M").to_string()),
            (
                "tm".to_owned(),
                time.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            ),
        ]
    }

    fn handle(&mut self, command: &str, reply_params: &Reply) -> Result<Vec<(String, String)>, ()> {
        if let Some(mut answer) = self.query(command) {
            let interface = reply_params.get("pi").unwrap_or("1");
            if command == "pccfgget"
                && let Some(code_page) = self.code_pages.get(interface)
            {
                answer.retain(|(key, _)| key != "cp");
                answer.push(("cp".to_owned(), code_page.clone()));
            }
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
                if nip.trim().is_empty() || nip.chars().count() > MAX_BUYER_NIP {
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
                if number(&params, "wa")? != value || !self.active_slot(slot) {
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
            "trftrln" if self.footer_open => Ok(Vec::new()),
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
