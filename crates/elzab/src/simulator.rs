//! Offline `ElzabSTX` peer on the real Link/Port boundary.
//!
//! Synthetic identity, header, clock and counters are not hardware qualification.
//! Commands are decoded from recorded frames, then effects precede faulted replies.
use crate::frame;
use fiscal::{
    cp1250, decimal,
    error::Error,
    link::{Candidate, Link, Port},
};
use std::{
    collections::{HashMap, VecDeque},
    io,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    LoseReply,
    CorruptReply,
    Disconnect,
    Device(u32),
    Transport(String),
    PaperOut,
    ForeignReply,
    Reply(Vec<u8>),
}
#[derive(Debug)]
struct State {
    model: String,
    firmware: String,
    serial: String,
    page: String,
    header: String,
    clock: String,
    vat: [String; 7],
    status: [u128; 3],
    frames: Vec<Vec<u8>>,
    faults: HashMap<String, Fault>,
    connected: bool,
    receipts: u32,
    reports: u32,
    transaction: bool,
    daily_pending: bool,
    gross: u64,
    advances: u64,
    paid: u64,
    fragment: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            model: "ELZAB CUBE online".to_owned(),
            firmware: "MD06.02".to_owned(),
            serial: "ABC1234567890".to_owned(),
            page: "CP1250".to_owned(),
            header: "Warsztat Łódź".to_owned(),
            clock: "261024120000".to_owned(),
            vat: ["23,00", "8,00", "5,00", "0,00", "=", "?", "?"].map(str::to_owned),
            status: [0, (1_u128 << 102) | (1_u128 << 98) | 4, 32],
            frames: vec![],
            faults: HashMap::new(),
            connected: false,
            receipts: 0,
            reports: 0,
            transaction: false,
            daily_pending: false,
            gross: 0,
            advances: 0,
            paid: 0,
            fragment: usize::MAX,
        }
    }
}
#[derive(Debug)]
pub struct Simulator {
    state: Arc<Mutex<State>>,
    address: &'static str,
}
impl Simulator {
    pub fn start() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            address: "elzab-simulator",
        }
    }

    pub const fn address(&self) -> &'static str {
        self.address
    }

    pub fn link(&self) -> Arc<dyn Link> {
        self.link_with(vec![Candidate {
            port: self.address().to_owned(),
            hint: None,
        }])
    }

    pub fn link_with(&self, candidates: Vec<Candidate>) -> Arc<dyn Link> {
        Arc::new(SimLink {
            state: Arc::clone(&self.state),
            candidates,
        })
    }

    pub fn frames(&self) -> Vec<Vec<u8>> {
        lock(&self.state).frames.clone()
    }

    pub fn fault(&self, command: &str, fault: Fault) {
        drop(lock(&self.state).faults.insert(command.to_owned(), fault));
    }

    pub fn set_identity(&self, model: &str, firmware: &str) {
        let mut state = lock(&self.state);
        model.clone_into(&mut state.model);
        firmware.clone_into(&mut state.firmware);
    }

    pub fn set_serial(&self, serial: &str) {
        serial.clone_into(&mut lock(&self.state).serial);
    }

    pub fn set_code_page(&self, page: &str) {
        page.clone_into(&mut lock(&self.state).page);
    }

    pub fn set_header(&self, header: &str) {
        header.clone_into(&mut lock(&self.state).header);
    }

    pub fn set_clock(&self, clock: &str) {
        clock.clone_into(&mut lock(&self.state).clock);
    }

    pub fn set_vat(&self, vat: [&str; 7]) {
        lock(&self.state).vat = vat.map(str::to_owned);
    }

    pub fn set_status(&self, one: u128, two: u128, three: u32) {
        lock(&self.state).status = [one, two, u128::from(three)];
    }

    pub fn set_fragment_size(&self, size: usize) {
        lock(&self.state).fragment = size.max(1);
    }

    pub fn receipts(&self) -> u32 {
        lock(&self.state).receipts
    }

    pub fn reports(&self) -> u32 {
        lock(&self.state).reports
    }

    pub fn transaction_open(&self) -> bool {
        lock(&self.state).transaction
    }

    pub fn daily_pending(&self) -> bool {
        lock(&self.state).daily_pending
    }
}
#[derive(Debug)]
struct SimLink {
    state: Arc<Mutex<State>>,
    candidates: Vec<Candidate>,
}
impl Link for SimLink {
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        if port != "elzab-simulator" {
            return Err(Error::Open {
                port: port.to_owned(),
                source: io::ErrorKind::NotFound.into(),
            });
        }
        let mut state = lock(&self.state);
        if state.connected {
            return Err(Error::Open {
                port: port.to_owned(),
                source: io::ErrorKind::PermissionDenied.into(),
            });
        }
        state.connected = true;
        Ok(Box::new(SimPort {
            state: Arc::clone(&self.state),
            pending: VecDeque::new(),
            disconnected: false,
        }))
    }

    fn candidates(&self) -> Vec<Candidate> {
        self.candidates.clone()
    }

    fn time_scale(&self) -> u32 {
        1000
    }
}
struct SimPort {
    state: Arc<Mutex<State>>,
    pending: VecDeque<u8>,
    disconnected: bool,
}
impl Drop for SimPort {
    fn drop(&mut self) {
        lock(&self.state).connected = false;
    }
}
impl Port for SimPort {
    fn write_all(&mut self, bytes: &[u8], _timeout: Duration) -> io::Result<()> {
        if self.disconnected {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut state = lock(&self.state);
        state.frames.push(bytes.to_vec());
        let (header, data) = frame::decode(bytes).map_err(io::Error::other)?;
        let id = header
            .split('\u{1d}')
            .next()
            .ok_or(io::ErrorKind::InvalidData)?;
        let body = cp1250::decode(data).ok_or(io::ErrorKind::InvalidData)?;
        let command = body
            .split('\u{1d}')
            .next()
            .ok_or(io::ErrorKind::InvalidData)?;
        let fault = state.faults.remove(command);
        let (status, reply) = match &fault {
            Some(Fault::Transport(status)) => (status.as_str(), String::new()),
            Some(Fault::Device(code)) => ("SA", code.to_string()),
            Some(Fault::PaperOut) => {
                state.transaction = false;
                state.status[2] = 2;
                ("SA", "450".to_owned())
            },
            _ => ("SA", execute(&mut state, &body)),
        };
        let mut reply = frame::encode(
            &format!("{id}\x1d{status}"),
            &cp1250::encode(&reply).ok_or(io::ErrorKind::InvalidData)?,
        )
        .map_err(io::Error::other)?;
        match fault {
            Some(Fault::LoseReply) => return Ok(()),
            Some(Fault::Disconnect) => {
                self.disconnected = true;
                return Ok(());
            },
            Some(Fault::CorruptReply) => {
                if let Some(byte) = reply.get_mut(1) {
                    *byte ^= 1;
                }
            },
            Some(Fault::ForeignReply) => {
                let foreign = frame::encode("999999999\x1dSA", b"\x1dBFOREIGN\x1dDBAD")
                    .map_err(io::Error::other)?;
                self.pending.extend(foreign);
            },
            Some(Fault::Reply(raw)) => reply = raw,
            Some(Fault::Device(_) | Fault::Transport(_) | Fault::PaperOut) | None => {},
        }
        self.pending.extend(reply);
        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        if self.disconnected {
            return Ok(0);
        }
        if self.pending.is_empty() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let length = buffer
            .len()
            .min(self.pending.len())
            .min(lock(&self.state).fragment);
        for byte in buffer.iter_mut().take(length) {
            *byte = self
                .pending
                .pop_front()
                .ok_or(io::ErrorKind::UnexpectedEof)?;
        }
        Ok(length)
    }
}
fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}
fn read(state: &State, body: &str) -> Option<String> {
    let mut fields = body.split('\u{1d}');
    let command = fields.next().unwrap_or_default();
    Some(match command {
        "XTR" => format!("\x1dB{}\x1dC172\x1dD{}", state.model, state.firmware),
        "FNR" => format!("\x1dB{}", state.serial),
        "XHR" if fields.next() == Some("w") => format!("\x1dwA{}", state.page),
        "ySR" => {
            let [one, mut two, mut three] = state.status;
            if state.transaction {
                two |= 1_u128 << 68;
            }
            if state.daily_pending {
                three |= 1;
            }
            format!("\x1dSA1\x1eB{one:X}\x1dSA2\x1eB{two:X}\x1dSA3\x1eB{three:X}")
        },
        "YYR" if fields.next() == Some("J") => {
            format!("\x1dJA{}\x1eB256", state.header.split('\n').count())
        },
        "yHR" => match fields.next() {
            Some("x") => "\x1dxi1234567890".to_owned(),
            Some(field) => field
                .strip_prefix("va")
                .and_then(|n| n.parse::<usize>().ok())
                .and_then(|n| n.checked_sub(1))
                .and_then(|n| state.header.split('\n').nth(n))
                .map_or_else(|| "1".to_owned(), |name| format!("\x1d{field}\x1eb{name}")),
            None => "1".to_owned(),
        },
        "FTR" => {
            let mut result = "\x1dA".to_owned();
            for (letter, rate) in ('A'..='G').zip(&state.vat) {
                if letter != 'A' {
                    result.push('\u{1e}');
                }
                result.push(letter);
                result.push_str(rate);
            }
            result
        },
        "XCR" => format!("\x1dA{}", state.clock),
        "SER" => format!("\x1dJ{}", state.receipts),
        "RNR" => format!("\x1dA{}", state.reports + 1),
        "LPC" => format!("\x1dB{}", if state.transaction { 483 } else { 0 }),
        _ => return None,
    })
}
fn execute(state: &mut State, body: &str) -> String {
    if let Some(reply) = read(state, body) {
        return reply;
    }
    let mut fields = body.split('\u{1d}');
    let command = fields.next().unwrap_or_default();
    match command {
        "LOR" => {
            if state.transaction {
                return "483".to_owned();
            }
            if fields.next() != Some("C0") {
                return "1".to_owned();
            }
            state.transaction = true;
            state.gross = 0;
            state.advances = 0;
            state.paid = 0;
            String::new()
        },
        "LNP" if state.transaction => String::new(),
        "LIT" if state.transaction => {
            let total = fields
                .next()
                .and_then(|f| f.strip_prefix('G'))
                .and_then(|f| sub(f, 'E'))
                .and_then(currency);
            match total.and_then(|n| state.gross.checked_add(n)) {
                Some(sum) => {
                    state.gross = sum;
                    String::new()
                },
                None => "464".to_owned(),
            }
        },
        "LDS" if state.transaction => {
            let amount = fields
                .next()
                .and_then(|f| f.strip_prefix("IAA1\x1fB"))
                .and_then(|s| s.split('\u{1f}').next())
                .and_then(currency);
            match amount
                .and_then(|n| state.advances.checked_add(n))
                .filter(|n| *n <= state.gross)
            {
                Some(sum) => {
                    state.advances = sum;
                    String::new()
                },
                None => "1000".to_owned(),
            }
        },
        "LBP" if state.transaction => {
            let amount = fields
                .next()
                .and_then(|f| f.strip_prefix('K'))
                .and_then(|f| sub(f, 'C'))
                .and_then(currency);
            match amount.and_then(|n| state.paid.checked_add(n)) {
                Some(sum) => {
                    state.paid = sum;
                    String::new()
                },
                None => "475".to_owned(),
            }
        },
        "LCL" if state.transaction => {
            let total = fields
                .next()
                .and_then(|f| f.strip_prefix("RA"))
                .and_then(currency);
            if total == Some(0) {
                return "1008".to_owned();
            }
            if total != Some(state.gross - state.advances) || total != Some(state.paid) {
                return "465".to_owned();
            }
            state.transaction = false;
            state.receipts += 1;
            String::new()
        },
        "LAL" => String::new(),
        "RRP" if body == "RRP\x1dI1\x1dJ1" => {
            state.daily_pending = true;
            String::new()
        },
        "RPP" if body.ends_with("\x1dS0") => {
            state.reports += 1;
            String::new()
        },
        _ => "1".to_owned(),
    }
}
fn sub(body: &str, tag: char) -> Option<&str> {
    body.split('\u{1e}').find_map(|f| f.strip_prefix(tag))
}
fn currency(value: &str) -> Option<u64> {
    decimal::parse(&value.replace(',', "."), 2)
}
