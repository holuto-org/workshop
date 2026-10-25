//! Exclusive printer connection and reply correlation.

use std::{
    fmt, io,
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};

use fiscal::{
    error::Error,
    link::{Connection, Link, Port},
};

use crate::frame::{self, ETX, MAX_FRAME, Reply, STX};

const READ_CHUNK: usize = 256;

pub struct Session {
    port: Box<dyn Port>,
    /// Bytes beyond the last frame, retained for the next reply.
    pending: Vec<u8>,
    #[cfg(any(test, feature = "simulator"))]
    time_scale: u32,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn open(link: &dyn Link, connection: &Connection) -> Result<Self, Error> {
        if connection.port.trim().is_empty() {
            return Err(Error::NoPort);
        }
        let session = Self::new(link.open(&connection.port, connection.baud_rate)?);
        #[cfg(any(test, feature = "simulator"))]
        let session = Self {
            time_scale: link.time_scale().max(1),
            ..session
        };
        Ok(session)
    }

    pub(crate) fn new(port: Box<dyn Port>) -> Self {
        Self {
            port,
            pending: Vec::new(),
            #[cfg(any(test, feature = "simulator"))]
            time_scale: 1,
        }
    }

    #[cfg(any(test, feature = "simulator"))]
    pub(crate) fn timeout(&self, duration: Duration) -> Duration {
        if self.time_scale == 1 {
            duration
        } else {
            (duration / self.time_scale).max(Duration::from_millis(100))
        }
    }

    #[cfg(not(any(test, feature = "simulator")))]
    #[expect(
        clippy::unused_self,
        reason = "production keeps the session API without a timing override"
    )]
    pub(crate) const fn timeout(&self, duration: Duration) -> Duration {
        duration
    }

    #[cfg(any(test, feature = "simulator"))]
    pub(crate) fn operator_wait_limit(&self, duration: Duration) -> Duration {
        self.port.operator_wait_limit(self.timeout(duration))
    }

    #[cfg(not(any(test, feature = "simulator")))]
    #[expect(
        clippy::unused_self,
        reason = "production keeps the session API without a timing override"
    )]
    pub(crate) const fn operator_wait_limit(&self, duration: Duration) -> Duration {
        duration
    }

    pub fn command(
        &mut self,
        command: &'static str,
        params: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Reply, Error> {
        let deadline = self.send(command, params, timeout)?;
        self.receive(command, deadline)
    }

    /// A late reply to another command is an error, not this command's success.
    pub(crate) fn receive(
        &mut self,
        command: &'static str,
        deadline: Instant,
    ) -> Result<Reply, Error> {
        loop {
            let (answered, reply) = self.next_frame(command, deadline)?;
            // Tokens mark replies to earlier asynchronous status queries.
            if reply.get("@").is_some() {
                continue;
            }
            let reply = reply.into_result(&answered)?;
            if answered != command {
                return Err(Error::Protocol(format!(
                    "Oczekiwano odpowiedzi {command}, otrzymano {answered}."
                )));
            }
            return Ok(reply);
        }
    }

    pub(crate) fn send(
        &mut self,
        command: &'static str,
        params: &[(&str, String)],
        timeout: Duration,
    ) -> Result<Instant, Error> {
        let timeout = self.timeout(timeout);
        tracing::debug!(
            command,
            timeout_ms = timeout.as_millis(),
            "Sending printer command once"
        );
        let frame = frame::encode(command, params)?;
        let deadline = Instant::now() + timeout;
        self.port
            .write_all(&frame, timeout)
            .map_err(|error| io_error(command, error))?;
        Ok(deadline)
    }

    /// Retains error codes and query tokens for correlation; `command` labels timeout errors.
    pub(crate) fn next_frame(
        &mut self,
        command: &'static str,
        deadline: Instant,
    ) -> Result<(String, Reply), Error> {
        let mut chunk = [0_u8; READ_CHUNK];
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == ETX) {
                let raw: Vec<u8> = self.pending.drain(..=end).collect();
                // Discard line noise before the last STX; CRC validates the frame.
                let Some(start) = raw.iter().rposition(|byte| *byte == STX) else {
                    continue;
                };
                return frame::decode_response(raw.get(start..).unwrap_or_default());
            }
            if self.pending.len() >= MAX_FRAME {
                return Err(Error::Protocol("Zbyt długa odpowiedź drukarki.".to_owned()));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout(command));
            }
            let read = match self.port.read(&mut chunk, remaining) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => result.map_err(|error| io_error(command, error))?,
            };
            self.pending
                .extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }
}

/// Echoed request token, `0000`–`9999` (spec p. 11).
pub(crate) fn token() -> String {
    static NEXT: AtomicU16 = AtomicU16::new(0);
    next_token(&NEXT)
}

fn next_token(next: &AtomicU16) -> String {
    let value = next.update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        (value + 1) % 10_000
    });
    format!("{value:04}")
}

fn io_error(command: &'static str, error: io::Error) -> Error {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        Error::Timeout(command)
    } else {
        Error::Io(error)
    }
}

#[cfg(test)]
mod tests;
