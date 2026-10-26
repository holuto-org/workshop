#![cfg(test)]

use futures_lite::future::block_on;
use slint::platform::update_timers_and_animations;
use std::{
    env, error::Error, fs, process::Command as ProcessCommand, sync::mpsc, thread, time::Duration,
};
use zbus::Connection;

use super::available;
use crate::ui::TrayIcon;

/// Lists items as `service/path`, like the KDE watcher.
#[derive(Debug)]
struct Watcher {
    host: bool,
    items: Vec<String>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&mut self, service: &str) {
        self.items.push(format!("{service}/StatusNotifierItem"));
    }

    #[zbus(property)]
    const fn is_status_notifier_host_registered(&self) -> bool {
        self.host
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.clone()
    }
}

/// Creates the application's icon the way `application::run` does.
///
/// Slint registers it with the watcher from its change handlers; running them here
/// makes the attempt synchronous.
fn show_icon() -> Result<TrayIcon, slint::PlatformError> {
    let tray = TrayIcon::new()?;
    tray.show()?;
    update_timers_and_animations();
    Ok(tray)
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture setup"
)]
fn tray_availability_requires_this_apps_icon_registered_with_a_tray_host()
-> Result<(), Box<dyn Error>> {
    if env::var_os("WORKSHOP_PRIVATE_TEST_BUS").is_none() {
        let temporary = tempfile::tempdir()?;
        let outcome = temporary.path().join("outcome");
        #[expect(
            clippy::disallowed_methods,
            reason = "The platform test needs a private D-Bus daemon, not the operator's tray host"
        )]
        let child = ProcessCommand::new("dbus-run-session")
            .arg("--")
            .arg(env::current_exe()?)
            .args([
                "--exact",
                concat!(
                    "tray::session_bus::",
                    "tray_availability_requires_this_apps_icon_registered_with_a_tray_host"
                ),
                "--nocapture",
            ])
            .env("WORKSHOP_PRIVATE_TEST_BUS", "1")
            .env("WORKSHOP_TRAY_OUTCOME", &outcome)
            .output()?;
        assert!(child.status.success(), "{child:?}");
        assert_eq!(
            fs::read_to_string(outcome)?,
            "absent; unregistered; registered; hostless; vanished; hung\n"
        );
        return Ok(());
    }
    let _address = env::var("DBUS_SESSION_BUS_ADDRESS")?;
    i_slint_backend_testing::init_integration_test_with_mock_time();
    block_on(async {
        assert!(
            !available().await,
            "a bus without StatusNotifierWatcher must keep the application visible"
        );
        // Autostart can win the login race against the desktop's watcher.
        let _early = show_icon()?;
        let watcher = Connection::session().await?;
        let _served = watcher
            .object_server()
            .at(
                "/StatusNotifierWatcher",
                Watcher {
                    host: true,
                    items: Vec::new(),
                },
            )
            .await?;
        watcher
            .request_name("org.kde.StatusNotifierWatcher")
            .await?;
        assert!(
            !available().await,
            "an icon created before the watcher never registered; hiding would strand the app"
        );
        let _registered = show_icon()?;
        assert!(
            available().await,
            "this application's icon registered with a tray host makes hiding safe"
        );
        watcher
            .object_server()
            .interface::<_, Watcher>("/StatusNotifierWatcher")
            .await?
            .get_mut()
            .await
            .host = false;
        assert!(
            !available().await,
            "a watcher without a registered tray host cannot show the icon"
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
            .spawn(move || sender.send(block_on(available())))?;
        assert_eq!(
            probed.recv_timeout(Duration::from_secs(10)),
            Ok(false),
            "an unresponsive watcher must not leave the close button doing nothing"
        );
        fs::write(
            env::var_os("WORKSHOP_TRAY_OUTCOME").ok_or("missing outcome path")?,
            "absent; unregistered; registered; hostless; vanished; hung\n",
        )?;
        Ok::<(), Box<dyn Error>>(())
    })
}
