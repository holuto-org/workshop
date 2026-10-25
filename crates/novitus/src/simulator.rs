//! Offline native Novitus TCP peer using the real Session and shared Link/Port.
//!
//! Read-only fixtures follow N:L795-L850,L1230-L1290,L1969-L1993,
//! L2073-L2112,L13380-L13386,L16080-L16101; raw status N:L383-L536.
//! Identity, clock, VAT and header are synthetic, not hardware qualification.

use crate::{
    frame,
    session::{DLE, DLE2, ENQ, identifier, no_request_checksum},
};
use fiscal::{
    cp1250,
    error::Error,
    link::{Candidate, Link, Port},
};
use std::{
    collections::HashMap,
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
    /// Exact wire bytes, allowing fragmented/coalesced and malformed envelopes.
    Reply(Vec<u8>),
}

#[derive(Debug)]
struct State {
    endpoint: String,
    dialect: String,
    header: String,
    model: String,
    firmware: String,
    clock: String,
    vat: [String; 7],
    unique: String,
    dle: u8,
    enq: u8,
    extended: String,
    frames: Vec<Vec<u8>>,
    faults: HashMap<String, Fault>,
    fragment: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            endpoint: "PC1".to_owned(),
            dialect: "1".to_owned(),
            header: "Warsztat Łódź".to_owned(),
            model: "NOVITUS HD ONLINE".to_owned(),
            firmware: "1.60".to_owned(),
            clock: "26;10;24;12;0;0".to_owned(),
            vat: ["23.00", "8.00", "5.00", "0.00", "98.99", "99.99", "99.99"].map(str::to_owned),
            unique: "ABC1234567890".to_owned(),
            dle: 0x74,
            enq: 0x6D,
            extended: "@s0;0;0;00000000;".to_owned(),
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

    pub fn set_dialect(&self, dialect: &str) {
        dialect.clone_into(&mut lock(&self.state).dialect);
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
            let (raw, response, fault, fragment) = {
                let mut state = lock(state);
                if first == 0x1B {
                    let Some(end) = pending.windows(2).position(|pair| pair == frame::END) else {
                        break;
                    };
                    let raw: Vec<_> = pending.drain(..end + 2).collect();
                    let Ok(payload) = frame::unframe(&raw) else {
                        return;
                    };
                    let Some(raw_body) = cp1250::decode(payload) else {
                        return;
                    };
                    let body = if no_request_checksum(&raw_body) {
                        raw_body
                    } else {
                        let Ok(payload) = frame::decode(&raw) else {
                            return;
                        };
                        let Some(body) = cp1250::decode(payload) else {
                            return;
                        };
                        body
                    };
                    let Some(command) = identifier(&body) else {
                        return;
                    };
                    let fault = state.faults.remove(command);
                    let response = answer(&body, command, &state);
                    (raw, response, fault, state.fragment)
                } else {
                    let raw = vec![pending.remove(0)];
                    let response = match first {
                        DLE => vec![state.dle],
                        ENQ => vec![state.enq],
                        DLE2 => frame::encode(&cp1250::encode(&state.extended).unwrap_or_default()),
                        _ => return,
                    };
                    (raw, response, None, state.fragment)
                }
            };
            lock(state).frames.push(raw);
            let response = match fault {
                Some(Fault::LoseReply) => continue,
                Some(Fault::Disconnect) => return,
                Some(Fault::Reply(raw)) => raw,
                Some(Fault::CorruptReply) => {
                    let mut raw = response;
                    if let Some(byte) = raw.get_mut(2) {
                        *byte ^= 1;
                    }
                    raw
                },
                None => response,
            };
            for chunk in response.chunks(fragment) {
                if stream.write_all(chunk).is_err() {
                    return;
                }
            }
        }
        if pending.len() >= frame::MAX_FRAME {
            return;
        }
    }
}
fn answer(body: &str, command: &str, state: &State) -> Vec<u8> {
    let (response, checksum, ack) = match body {
        "3#e" => return frame::query(b"0#Z#e"),
        "30#v" => (
            format!("1#R{}/{}", state.model, state.firmware),
            false,
            false,
        ),
        "50$i" => (format!("50$I{}\r", state.endpoint), true, true),
        "0;1^c5/5/" | "^c" => (format!("1^c5\r{}\r", state.dialect), true, true),
        "#c" => (format!("1#C{}", state.clock), false, false),
        "#n" => ("1#E0".to_owned(), false, false),
        "^u" => (format!("1#U{}", state.header), true, true),
        "23#s" => (
            format!(
                "2#X0;1;0;1;1;0;26;10;24/{}/0/0/0/0/0/0/0/0/0/{}",
                state.vat.join("/"),
                state.unique
            ),
            true,
            false,
        ),
        _ => return frame::query(format!("82#Z{command}").as_bytes()),
    };
    let bytes = cp1250::encode(&response).unwrap_or_default();
    let mut raw = if checksum {
        frame::encode(&bytes)
    } else {
        frame::query(&bytes)
    };
    if ack {
        raw.extend_from_slice(&frame::query(format!("0#Z{command}").as_bytes()));
    }
    raw
}

#[cfg(test)]
mod tests;
