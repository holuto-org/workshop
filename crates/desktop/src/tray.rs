#[cfg(not(all(unix, not(target_vendor = "apple"))))]
use std::future::{Ready, ready};

/// Slint misses Linux tray-host loss after shell restarts; recheck before hiding.
/// A watcher needs a registered host to display the icon.
#[cfg(all(unix, not(target_vendor = "apple")))]
pub(crate) async fn available() -> bool {
    use std::time::Duration;

    use zbus::{connection, fdo::PropertiesProxy, names::InterfaceName};

    let check = async {
        // An unresponsive watcher must not leave the close button doing nothing.
        let connection = connection::Builder::session()?
            .method_timeout(Duration::from_secs(2))
            .build()
            .await?;
        let watcher = PropertiesProxy::builder(&connection)
            .destination("org.kde.StatusNotifierWatcher")?
            .path("/StatusNotifierWatcher")?
            .build()
            .await?;
        let registered = watcher
            .get(
                InterfaceName::from_static_str("org.kde.StatusNotifierWatcher")?,
                "IsStatusNotifierHostRegistered",
            )
            .await?;
        Ok::<bool, zbus::Error>(bool::try_from(registered)?)
    };
    check.await.unwrap_or(false)
}

#[cfg(not(all(unix, not(target_vendor = "apple"))))]
pub(crate) fn available() -> Ready<bool> {
    ready(true)
}

#[cfg(all(test, target_os = "linux"))]
#[path = "tray/session_bus.rs"]
mod session_bus;
