//! Offline native Novitus TCP peer using the real Session and shared Link/Port.
//!
//! Read-only fixtures follow the native protocol's documented replies and raw status.
//! Identity, clock, VAT and header are synthetic, not hardware qualification.

use crate::{
    frame,
    session::{DLE, DLE2, ENQ, identifier, no_request_checksum},
};
use fiscal::{
    cp1250, decimal,
    error::Error,
    link::{Candidate, Link, Port},
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    io::{self, Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    LoseReply,
    CorruptReply,
    Disconnect,
    /// Matched refusal, without executing the command.
    Refuse(u32),
    /// Execute once, hold its ACK until `resume`, and expose DLE paper/battery.
    Wait,
    /// Exact wire bytes, allowing fragmented/coalesced and malformed envelopes.
    Reply(Vec<u8>),
}

#[derive(Debug)]
struct State {
    endpoint: String,
    error_mode: u8,
    options: BTreeMap<u32, String>,
    header: String,
    model: String,
    firmware: String,
    clock: String,
    vat: [String; 7],
    unique: String,
    dle: u8,
    enq: u8,
    extended: String,
    fiscal_mode: u8,
    capabilities_attr1: u32,
    transaction: bool,
    ending: bool,
    receipts: u32,
    reports: u32,
    waiting: bool,
    gross: u64,
    paid: u64,
    frames: Vec<Vec<u8>>,
    faults: HashMap<String, Fault>,
    fragment: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            endpoint: "PC1".to_owned(),
            error_mode: 0,
            options: [5, 91, 92, 93, 94]
                .map(|option| (option, "1".to_owned()))
                .into(),
            header: "Warsztat Łódź".to_owned(),
            model: "NOVITUS HD ONLINE".to_owned(),
            firmware: "1.60".to_owned(),
            clock: "26;10;24;12;0;0".to_owned(),
            vat: ["23.00", "8.00", "5.00", "0.00", "98.99", "99.99", "99.99"].map(str::to_owned),
            unique: "ABC1234567890".to_owned(),
            dle: 0x74,
            enq: 0x6D,
            extended: "@s0;0;0;00000000;".to_owned(),
            fiscal_mode: 1,
            capabilities_attr1: 0x0200,
            transaction: false,
            ending: false,
            receipts: 0,
            reports: 0,
            waiting: false,
            gross: 0,
            paid: 0,
            frames: Vec::new(),
            faults: HashMap::new(),
            fragment: usize::MAX,
        }
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
        reason = "test infrastructure requires local sockets and a thread"
    )]
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Novitus simulator");
        let address = listener
            .local_addr()
            .expect("simulator address")
            .to_string();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("Novitus simulator".to_owned())
            .spawn({
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                move || serve(&listener, &state, &stop)
            })
            .expect("start simulator thread");
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
        self.link_with(vec![Candidate {
            port: self.address.clone(),
            hint: None,
        }])
    }

    pub fn link_with(&self, candidates: Vec<Candidate>) -> Arc<dyn Link> {
        Arc::new(SimLink {
            address: self.address.clone(),
            candidates,
            connected: Arc::clone(&self.connected),
        })
    }

    pub fn frames(&self) -> Vec<Vec<u8>> {
        lock(&self.state).frames.clone()
    }

    pub fn set_endpoint(&self, endpoint: &str) {
        endpoint.clone_into(&mut lock(&self.state).endpoint);
    }

    /// Power-on defaults to mode 0; normal driver sessions explicitly select mode 3.
    pub fn set_error_mode(&self, mode: u8) {
        lock(&self.state).error_mode = mode;
    }

    pub fn set_dialect(&self, dialect: &str) {
        self.set_config_option(5, dialect);
    }

    pub fn set_config_option(&self, option: u32, value: &str) {
        drop(lock(&self.state).options.insert(option, value.to_owned()));
    }

    pub fn set_header(&self, header: &str) {
        header.clone_into(&mut lock(&self.state).header);
    }

    pub fn set_identity(&self, model: &str, firmware: &str) {
        let mut state = lock(&self.state);
        model.clone_into(&mut state.model);
        firmware.clone_into(&mut state.firmware);
    }

    /// Exact six semicolon-separated clock fields, not host local time.
    pub fn set_clock(&self, clock: &str) {
        clock.clone_into(&mut lock(&self.state).clock);
    }

    pub fn set_vat(&self, vat: [&str; 7]) {
        lock(&self.state).vat = vat.map(str::to_owned);
    }

    pub fn set_status(&self, dle: u8, enq: u8, extended: &str) {
        let mut state = lock(&self.state);
        state.dle = dle;
        state.enq = enq;
        extended.clone_into(&mut state.extended);
    }

    pub fn set_fragment_size(&self, size: usize) {
        lock(&self.state).fragment = size.max(1);
    }

    pub fn fault(&self, command: &str, fault: Fault) {
        drop(lock(&self.state).faults.insert(command.to_owned(), fault));
    }

    pub fn set_fiscal_mode(&self, mode: u8) {
        lock(&self.state).fiscal_mode = mode;
    }

    pub fn set_full_cp1250(&self, enabled: bool) {
        lock(&self.state).capabilities_attr1 = if enabled { 0x0200 } else { 0 };
    }

    pub fn receipts(&self) -> u32 {
        lock(&self.state).receipts
    }

    pub fn reports(&self) -> u32 {
        lock(&self.state).reports
    }

    pub fn resume(&self) {
        let mut state = lock(&self.state);
        state.waiting = false;
        state.dle = 0x74;
    }
}
impl Drop for Simulator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        drop(TcpStream::connect(&self.address));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

#[derive(Debug)]
struct SimLink {
    address: String,
    candidates: Vec<Candidate>,
    connected: Arc<AtomicBool>,
}
impl Link for SimLink {
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        if port != self.address {
            return Err(Error::Open {
                port: port.to_owned(),
                source: io::ErrorKind::NotFound.into(),
            });
        }
        if self.connected.swap(true, Ordering::SeqCst) {
            return Err(Error::Open {
                port: port.to_owned(),
                source: io::ErrorKind::PermissionDenied.into(),
            });
        }
        let stream = TcpStream::connect(port).and_then(|stream| {
            stream.set_nodelay(true)?;
            Ok(stream)
        });
        match stream {
            Ok(stream) => Ok(Box::new(SimPort {
                stream,
                connected: Arc::clone(&self.connected),
            })),
            Err(source) => {
                self.connected.store(false, Ordering::SeqCst);
                Err(Error::Open {
                    port: port.to_owned(),
                    source,
                })
            },
        }
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.candidates.clone()
    }

    fn time_scale(&self) -> u32 {
        1000
    }
}
struct SimPort {
    stream: TcpStream,
    connected: Arc<AtomicBool>,
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
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
fn serve(listener: &TcpListener, state: &Mutex<State>, stop: &AtomicBool) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(stream) = stream {
            serve_session(stream, state, stop);
        }
    }
}
fn serve_session(mut stream: TcpStream, state: &Mutex<State>, stop: &AtomicBool) {
    if stream
        .set_read_timeout(Some(Duration::from_millis(10)))
        .is_err()
        || stream.set_nodelay(true).is_err()
    {
        return;
    }
    let mut pending = Vec::new();
    let mut chunk = [0; 256];
    let mut held_response = None;
    while !stop.load(Ordering::SeqCst) {
        match stream.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => pending.extend_from_slice(chunk.get(..read).unwrap_or_default()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            },
            Err(_) => return,
        }
        while let Some(first) = pending.first().copied() {
            if first == 0x02 {
                if !ignore_foreign(&mut pending, state) {
                    break;
                }
                continue;
            }
            let (raw, response, fault, fragment) = {
                let mut state = lock(state);
                if first == 0x1B {
                    let Some(end) = pending.windows(2).position(|pair| pair == frame::END) else {
                        break;
                    };
                    let raw: Vec<_> = pending.drain(..end + 2).collect();
                    let Some(body) = request_body(&raw) else {
                        return;
                    };
                    let Some(command) = identifier(&body) else {
                        return;
                    };
                    let fault = state
                        .faults
                        .remove(&body)
                        .or_else(|| state.faults.remove(command));
                    let response = if let Some(Fault::Refuse(code)) = fault {
                        frame::query(format!("{code}#Z{command}").as_bytes())
                    } else {
                        answer(&body, command, &mut state)
                    };
                    if matches!(fault, Some(Fault::Wait)) {
                        state.waiting = true;
                        state.dle = 0x76;
                    }
                    (raw, response, fault, state.fragment)
                } else {
                    let raw = vec![pending.remove(0)];
                    let Some(response) = realtime_answer(first, &state) else {
                        return;
                    };
                    (raw, response, None, state.fragment)
                }
            };
            lock(state).frames.push(raw);
            let response = match fault {
                Some(Fault::Wait) => {
                    held_response = Some(response);
                    continue;
                },
                Some(Fault::LoseReply) => continue,
                Some(Fault::Disconnect) => return,
                Some(Fault::Reply(raw)) => raw,
                Some(Fault::CorruptReply) => corrupt_response(response),
                Some(Fault::Refuse(_)) | None => response,
            };
            if response
                .chunks(fragment)
                .any(|chunk| stream.write_all(chunk).is_err())
            {
                return;
            }
            if !lock(state).waiting
                && let Some(response) = held_response.take()
                && stream.write_all(&response).is_err()
            {
                return;
            }
        }
        if pending.len() >= frame::MAX_FRAME {
            return;
        }
    }
}

/// Foreign STX envelopes are inert, but remain evidence of discovery traffic.
fn ignore_foreign(pending: &mut Vec<u8>, state: &Mutex<State>) -> bool {
    let Some(end) = pending.iter().position(|byte| *byte == 0x03) else {
        return false;
    };
    lock(state).frames.push(pending.drain(..=end).collect());
    true
}

fn corrupt_response(mut response: Vec<u8>) -> Vec<u8> {
    if let Some(byte) = response.get_mut(2) {
        *byte ^= 1;
    }
    response
}

fn request_body(raw: &[u8]) -> Option<String> {
    let raw_body = cp1250::decode(frame::unframe(raw).ok()?)?;
    if no_request_checksum(&raw_body) {
        Some(raw_body)
    } else {
        cp1250::decode(frame::decode(raw).ok()?)
    }
}
fn realtime_answer(byte: u8, state: &State) -> Option<Vec<u8>> {
    Some(match byte {
        DLE => vec![state.dle],
        ENQ => vec![state.enq | if state.transaction { 2 } else { 0 }],
        DLE2 => {
            let extended = if state.transaction || state.ending {
                format!("@s0;{};{};FFFFFFFF;", state.receipts, state.receipts)
            } else {
                state.extended.clone()
            };
            frame::encode(&cp1250::encode(&extended)?)
        },
        _ => return None,
    })
}
fn answer(body: &str, command: &str, state: &mut State) -> Vec<u8> {
    let (response, checksum, ack) = match body {
        "3#e" => {
            state.error_mode = 3;
            return frame::query(b"0#Z#e");
        },
        "30#v" => (
            format!("1#R{}/{}", state.model, state.firmware),
            false,
            false,
        ),
        "50$i" => (format!("50$I{}\r", state.endpoint), true, true),
        "11$i" => (
            format!(
                "11$I{}\r2020-01-01 12:00:00\r1234567890\rREG1\rMEM1\r0\r0\r0\r",
                state.fiscal_mode
            ),
            true,
            true,
        ),
        "52$i" => (
            format!(
                concat!(
                    "52$I{:08X}\r00000000\r00000000\r00000000\r",
                    "00000000\r00000000\r00000000\r00000000\r"
                ),
                state.capabilities_attr1
            ),
            true,
            true,
        ),
        "0;0?l" => (
            format!(
                concat!(
                    "0?l2/0/{}/2026-10-24 12:00:00\r{}/\r\r\r3/0/5/{}/0/0/\r{}/0/0/0/",
                    "{}/0/0/0/0/0/0/0/0/0/0/\r\r\r\r\r\r\r0/0/0/"
                ),
                state.receipts, state.receipts, state.gross, state.gross, state.gross
            ),
            true,
            true,
        ),
        _ if command == "^c" => {
            let Some(response) = configuration_reply(body, &state.options) else {
                return frame::query(b"4#Z^c");
            };
            (response, true, true)
        },
        "#c" => (format!("1#C{}", state.clock), false, false),
        "#n" => ("1#E0".to_owned(), false, false),
        "^u" => (format!("1#U{}", state.header), true, true),
        "23#s" => (
            format!(
                "2#X0;{};0;1;1;0;26;10;24/{}/0/0/0/0/0/0/0/0/0/{}",
                u8::from(state.fiscal_mode != 0),
                state.vat.join("/"),
                state.unique
            ),
            true,
            false,
        ),
        _ if matches!(
            command,
            "$h" | "$l" | "$z" | "$b" | "$y" | "$w" | "@z" | "#r" | "#o"
        ) =>
        {
            let code = fiscal_command(body, command, state);
            return frame::query(format!("{code}#Z{command}").as_bytes());
        },
        _ => return frame::query(format!("82#Z{command}").as_bytes()),
    };
    let bytes = cp1250::encode(&response).unwrap_or_default();
    let mut raw = if checksum {
        frame::encode(&bytes)
    } else {
        frame::query(&bytes)
    };
    if ack && state.error_mode == 3 {
        raw.extend_from_slice(&frame::query(format!("0#Z{command}").as_bytes()));
    }
    raw
}

fn fiscal_command(body: &str, command: &str, state: &mut State) -> u32 {
    match command {
        "$h" if body == "0$h" && !state.transaction => {
            state.transaction = true;
            state.ending = false;
            state.gross = 0;
            state.paid = 0;
        },
        "$l" if state.transaction && !state.ending => {
            let Some(fields) = body.split_once("$l").map(|(_, tail)| tail) else {
                return 4;
            };
            let fields: Vec<_> = fields.split('\r').collect();
            let Some(amounts) = fields.get(2) else {
                return 4;
            };
            let Some(total) = amounts
                .split('/')
                .nth(2)
                .and_then(|amount| decimal::parse(amount, 2))
            else {
                return 20;
            };
            state.gross += total;
        },
        "@z" if state.transaction && !state.ending => {
            let Some(amounts) = body.rsplit('\r').next() else {
                return 4;
            };
            let Some(value) = amounts
                .split('/')
                .next()
                .and_then(|amount| decimal::parse(amount, 2))
            else {
                return 4;
            };
            let Some(gross) = state.gross.checked_sub(value) else {
                return 20;
            };
            state.gross = gross;
        },
        "$b" if state.transaction && !state.ending => {
            let Some(value) = body
                .split_once("$b")
                .and_then(|(_, tail)| tail.split('/').next())
                .and_then(|amount| decimal::parse(amount, 2))
            else {
                return 19;
            };
            state.paid += value;
        },
        "$y" if state.transaction && !state.ending => {
            let Some(tail) = body.rsplit('\r').next() else {
                return 4;
            };
            let Some(total) = tail
                .split('/')
                .next()
                .and_then(|amount| decimal::parse(amount, 2))
            else {
                return 27;
            };
            if total != state.gross || state.paid != total {
                return 27;
            }
            state.ending = true;
        },
        "$z" if body.starts_with("100;1$z") && state.transaction && !state.ending => {},
        "$z" if body == "12$z" && state.ending => {},
        "$w" if body.starts_with("200;0$w") && state.ending => {},
        "$z" if body == "28;0$z" && state.ending => {
            state.receipts += 1;
            state.transaction = false;
            state.ending = false;
        },
        "#r" | "#o" if !state.transaction => {
            state.reports += 1;
        },
        _ => return 21,
    }
    0
}

/// Full and bounded configuration reads.
fn configuration_reply(body: &str, options: &BTreeMap<u32, String>) -> Option<String> {
    let (first, last) = if body == "^c" {
        (0, u32::MAX)
    } else {
        let range = body.strip_prefix("0;1^c")?.strip_suffix('/')?;
        let (first, last) = range.split_once('/')?;
        (first.parse::<u32>().ok()?, last.parse::<u32>().ok()?)
    };
    if first > last {
        return None;
    }
    let options = options.range(first..=last);
    let mut response = format!("{}^c", options.clone().count());
    for (option, value) in options {
        let _written = write!(response, "{option}\r{value}\r");
    }
    Some(response)
}

#[cfg(test)]
mod tests;
