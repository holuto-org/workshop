use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    time::Duration,
};

use super::Session;
use crate::{
    Error,
    frame::{self, encode},
    simulator::TcpPort,
    text,
};

const TIMEOUT: Duration = Duration::from_millis(300);

/// A device that answers only with the bytes the test writes to it.
trait Peer {
    fn send(&mut self, bytes: &[u8]);
    /// Everything that arrived within `wait`.
    fn received(&mut self, wait: Duration) -> Vec<u8>;
}

impl Peer for TcpStream {
    fn send(&mut self, bytes: &[u8]) {
        self.write_all(bytes).unwrap();
    }

    fn received(&mut self, wait: Duration) -> Vec<u8> {
        self.set_read_timeout(Some(wait)).unwrap();
        let mut received = Vec::new();
        let mut chunk = [0_u8; 4096];
        while let Ok(read) = self.read(&mut chunk) {
            if read == 0 {
                break;
            }
            received.extend_from_slice(&chunk[..read]);
        }
        received
    }
}

fn tcp_pair() -> (TcpStream, Session) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let driver = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    (peer, Session::new(Box::new(TcpPort(driver))))
}

fn reply(payload: &str) -> Vec<u8> {
    frame::wrap(&text::encode(payload).unwrap())
}

/// A lost reply surfaces as a timeout; the fiscal command is never sent a second time.
fn lost_reply_never_retries(peer: &mut dyn Peer, session: &mut Session) {
    let trinit = encode("trinit", &[("bm", "0".to_owned())]).unwrap();
    let result = session.command("trinit", &[("bm", "0".to_owned())], TIMEOUT);
    assert!(
        matches!(result, Err(Error::Timeout("trinit"))),
        "{result:?}"
    );
    assert_eq!(peer.received(Duration::from_millis(200)), trinit);
}

/// Noise before a frame is skipped, but a late reply to another command is never a success.
fn foreign_reply_is_not_a_success(peer: &mut dyn Peer, session: &mut Session) {
    let mut noisy = b"\x00\xFF".to_vec();
    noisy.extend(reply("getrealid\tnmX\t"));
    peer.send(&noisy);
    assert_eq!(
        session
            .command("getrealid", &[], TIMEOUT)
            .unwrap()
            .get("nm"),
        Some("X")
    );
    peer.send(&reply("scnt\tbt1\t"));
    let result = session.command("trend", &[("to", "1".to_owned())], TIMEOUT);
    let Err(Error::Protocol(message)) = result else {
        panic!("{result:?}")
    };
    assert!(message.contains("Oczekiwano odpowiedzi trend"), "{message}");
}

#[test]
fn socket_lost_reply_never_retries() {
    let (mut peer, mut session) = tcp_pair();
    lost_reply_never_retries(&mut peer, &mut session);
}

#[test]
fn socket_foreign_reply_is_not_a_success() {
    let (mut peer, mut session) = tcp_pair();
    foreign_reply_is_not_a_success(&mut peer, &mut session);
}

/// The transport the printer actually uses: a terminal with termios timeouts and partial reads.
#[cfg(unix)]
mod pty {
    use std::{
        io::{Read as _, Write as _},
        time::Duration,
    };

    use serialport::{SerialPort as _, TTYPort};

    use super::{Peer, Session, foreign_reply_is_not_a_success, lost_reply_never_retries};
    use crate::session::SerialStream;

    impl Peer for TTYPort {
        fn send(&mut self, bytes: &[u8]) {
            self.write_all(bytes).unwrap();
        }

        fn received(&mut self, wait: Duration) -> Vec<u8> {
            self.set_timeout(wait).unwrap();
            let mut received = Vec::new();
            let mut chunk = [0_u8; 4096];
            while let Ok(read) = self.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                received.extend_from_slice(&chunk[..read]);
            }
            received
        }
    }

    fn pair() -> (TTYPort, Session) {
        let (master, slave) = TTYPort::pair().unwrap();
        (
            master,
            Session::new(Box::new(SerialStream(Box::new(slave)))),
        )
    }

    #[test]
    fn lost_reply_never_retries_on_a_terminal() {
        let (mut peer, mut session) = pair();
        lost_reply_never_retries(&mut peer, &mut session);
    }

    #[test]
    fn foreign_reply_is_not_a_success_on_a_terminal() {
        let (mut peer, mut session) = pair();
        foreign_reply_is_not_a_success(&mut peer, &mut session);
    }
}
