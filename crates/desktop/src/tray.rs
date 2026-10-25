#[cfg(not(all(unix, not(target_vendor = "apple"))))]
use std::future::{Ready, ready};

/// Whether closing may hide the window: a host must show this process's icon.
///
/// Slint creates the icon once and gives up when no watcher exists yet (autostart can
/// win the login race), and misses host loss after shell restarts.
#[cfg(all(unix, not(target_vendor = "apple")))]
pub(crate) async fn available() -> bool {
    use std::{process, time::Duration};

    use zbus::{
        connection,
        fdo::{DBusProxy, PropertiesProxy},
        names::{BusName, InterfaceName},
    };

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
        let interface = InterfaceName::from_static_str("org.kde.StatusNotifierWatcher")?;
        let host = watcher
            .get(interface.clone(), "IsStatusNotifierHostRegistered")
            .await?;
        if !bool::try_from(host)? {
            return Ok(false);
        }
        let items: Vec<String> = watcher
            .get(interface, "RegisteredStatusNotifierItems")
            .await?
            .try_into()?;
        let bus = DBusProxy::new(&connection).await?;
        for item in &items {
            // `service/path`; GNOME lists the unique name instead of the requested one.
            let service = item
                .split_once('/')
                .map_or(item.as_str(), |(service, _)| service);
            if let Ok(service) = BusName::try_from(service)
                && bus
                    .get_connection_unix_process_id(service)
                    .await
                    .is_ok_and(|owner| owner == process::id())
            {
                return Ok(true);
            }
        }
        Ok::<bool, zbus::Error>(false)
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
