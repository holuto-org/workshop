//! Bounded admission and detached SDK ownership keep telemetry HTTP and Drop joins
//! from blocking application shutdown.
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread::{self, Thread},
    time::{Duration, Instant},
};

use sentry::{
    Envelope, Transport, TransportFactory, TransportOptions, transports::UreqHttpTransportOptions,
};
use ureq::{
    Agent, Body, SendBody,
    http::{Request, Response},
    middleware::MiddlewareNext,
    tls::{RootCerts, TlsConfig},
};

use super::ENABLED;

pub(super) struct HttpTransport;

impl TransportFactory for HttpTransport {
    fn create_transport_with_options(&self, options: TransportOptions) -> Arc<dyn Transport> {
        let capacity = options.transport_channel_capacity.map_or(30, Into::into);
        Arc::new(DetachedTransport::spawn(
            move || {
                let config = Agent::config_builder()
                    .proxy(None)
                    .max_redirects(0)
                    .timeout_global(Some(Duration::from_secs(3)))
                    .middleware(consent)
                    .tls_config(
                        TlsConfig::builder()
                            .root_certs(RootCerts::PlatformVerifier)
                            .build(),
                    )
                    .build();
                UreqHttpTransportOptions::from(options)
                    .with_agent(Agent::new_with_config(config))
                    .build()
            },
            capacity,
        ))
    }
}

fn consent(
    request: Request<SendBody<'_>>,
    next: MiddlewareNext<'_>,
) -> Result<Response<Body>, ureq::Error> {
    // Recheck on the SDK HTTP thread, including envelopes queued before opt-out.
    if !ENABLED.load(Ordering::SeqCst) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Telemetry disabled").into());
    }
    next.handle(request)
}

#[derive(Clone, Copy)]
struct Deadline {
    started: Instant,
    timeout: Duration,
}

impl Deadline {
    fn new(timeout: Duration) -> Self {
        Self {
            started: Instant::now(),
            timeout,
        }
    }

    fn remaining(&self) -> Duration {
        self.timeout.saturating_sub(self.started.elapsed())
    }
}

#[expect(
    clippy::large_enum_variant,
    reason = "The queue mainly carries envelopes; boxing each would add an allocation"
)]
enum Task {
    Envelope(Envelope),
    Flush(Deadline, Sender<bool>),
    Shutdown(Deadline, Sender<bool>),
}

struct DetachedTransport {
    #[expect(
        clippy::disallowed_types,
        reason = "Bounded telemetry memory; only nonblocking try_send is used"
    )]
    sender: mpsc::SyncSender<Task>,
    closed: Arc<AtomicBool>,
    owner: Option<Thread>,
}

impl DetachedTransport {
    fn spawn<F, T>(create: F, capacity: usize) -> Self
    where
        F: FnOnce() -> T + Send + 'static,
        T: Transport,
    {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let closed = Arc::new(AtomicBool::new(false));
        let owner_closed = Arc::clone(&closed);
        let owner = thread::Builder::new()
            .name("telemetry-owner".into())
            .spawn(move || own_transport(create(), &receiver, &owner_closed))
            .ok()
            .map(|handle| {
                let owner = handle.thread().clone();
                // Telemetry may detach; the fiscal Worker must not.
                drop(handle);
                owner
            });
        Self {
            sender,
            closed,
            owner,
        }
    }

    fn wake(&self) {
        if let Some(owner) = &self.owner {
            owner.unpark();
        }
    }

    fn acknowledge(&self, deadline: &Deadline, receiver: &Receiver<bool>) -> bool {
        self.wake();
        receiver.recv_timeout(deadline.remaining()).unwrap_or(false)
    }
}

impl Transport for DetachedTransport {
    fn send_envelope(&self, envelope: Envelope) {
        if !self.closed.load(Ordering::SeqCst) {
            let _overflow_or_disconnected = self.sender.try_send(Task::Envelope(envelope));
            self.wake();
        }
    }

    fn flush(&self, timeout: Duration) -> bool {
        let deadline = Deadline::new(timeout);
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        let (sender, receiver) = mpsc::channel();
        if self.sender.try_send(Task::Flush(deadline, sender)).is_err() {
            return false;
        }
        self.acknowledge(&deadline, &receiver)
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        let deadline = Deadline::new(timeout);
        if self.closed.swap(true, Ordering::SeqCst) {
            return false;
        }
        let (sender, receiver) = mpsc::channel();
        if self
            .sender
            .try_send(Task::Shutdown(deadline, sender))
            .is_err()
        {
            // Wake the owner to observe closure even when the shutdown task cannot fit.
            self.wake();
            return false;
        }
        self.acknowledge(&deadline, &receiver)
    }
}

impl Drop for DetachedTransport {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        self.wake();
    }
}

fn own_transport<T: Transport>(transport: T, receiver: &Receiver<Task>, closed: &AtomicBool) {
    loop {
        match receiver.try_recv() {
            Ok(Task::Envelope(envelope)) => transport.send_envelope(envelope),
            Ok(Task::Flush(deadline, acknowledge)) => {
                let remaining = deadline.remaining();
                let flushed = !remaining.is_zero() && transport.flush(remaining);
                let _caller_gone = acknowledge.send(flushed);
            },
            Ok(Task::Shutdown(deadline, acknowledge)) => {
                let remaining = deadline.remaining();
                let stopped = !remaining.is_zero() && transport.shutdown(remaining);
                // SDK Drop may join HTTP; acknowledge success only after it finishes.
                drop(transport);
                let _caller_gone = acknowledge.send(stopped);
                return;
            },
            Err(TryRecvError::Empty) => {
                if closed.load(Ordering::SeqCst) {
                    return;
                }
                // unpark retains a token, preventing a send/park race.
                thread::park();
            },
            Err(TryRecvError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
#[path = "transport/tests.rs"]
mod tests;
