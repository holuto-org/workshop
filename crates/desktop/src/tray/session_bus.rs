#![cfg(test)]

use std::{
    env,
    error::Error,
    process::Command as ProcessCommand,
    sync::{Arc, mpsc},
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::Duration,
};
use zbus::Connection;

use super::available;

#[derive(Debug)]
struct Watcher(bool);

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    #[zbus(property)]
    const fn is_status_notifier_host_registered(&self) -> bool {
        self.0
    }
}

struct ThreadWake(Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
fn run<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn tray_availability_requires_a_registered_status_notifier_host() -> Result<(), Box<dyn Error>> {
    if env::var_os("WORKSHOP_PRIVATE_TEST_BUS").is_none() {
        #[expect(
            clippy::disallowed_methods,
            reason = "The platform test needs a private D-Bus daemon, not the operator's tray host"
        )]
        let child = ProcessCommand::new("dbus-run-session")
            .arg("--")
            .arg(env::current_exe()?)
            .args([
                "--exact",
                "tray::session_bus::tray_availability_requires_a_registered_status_notifier_host",
                "--nocapture",
            ])
            .env("WORKSHOP_PRIVATE_TEST_BUS", "1")
            .output()?;
        assert!(
            child.status.success() && String::from_utf8_lossy(&child.stdout).contains(" 1 passed"),
            "isolated bus test failed: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        return Ok(());
    }
    let _address = env::var("DBUS_SESSION_BUS_ADDRESS")?;
    run(async {
        assert!(
            !available().await,
            "a bus without StatusNotifierWatcher must keep the application visible"
        );
        let watcher = Connection::session().await?;
        let _served = watcher
            .object_server()
            .at("/StatusNotifierWatcher", Watcher(false))
            .await?;
        watcher
            .request_name("org.kde.StatusNotifierWatcher")
            .await?;
        assert!(
            !available().await,
            "a watcher without a registered tray host cannot show the icon"
        );
        watcher
            .object_server()
            .interface::<_, Watcher>("/StatusNotifierWatcher")
            .await?
            .get_mut()
            .await
            .0 = true;
        assert!(
            available().await,
            "a watcher with a registered tray host makes tray hiding safe"
        );
        let released = watcher
            .release_name("org.kde.StatusNotifierWatcher")
            .await?;
        assert!(released, "the fake watcher relinquishes its bus name");
        assert!(
            !available().await,
            "a vanished host must no longer allow hiding"
        );
        // A watcher that owns the name but never answers, like a frozen shell extension.
        let hung = Connection::session().await?;
        hung.request_name("org.kde.StatusNotifierWatcher").await?;
        let (sender, probed) = mpsc::channel();
        let _probe = thread::Builder::new()
            .name("tray probe".into())
            .spawn(move || sender.send(run(available())))?;
        assert_eq!(
            probed.recv_timeout(Duration::from_secs(10)),
            Ok(false),
            "an unresponsive watcher must not leave the close button doing nothing"
        );
        Ok::<(), Box<dyn Error>>(())
    })
}
