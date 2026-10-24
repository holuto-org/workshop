#[cfg(not(all(unix, not(target_vendor = "apple"))))]
use std::future::{Ready, ready};

/// Slint reports backend creation errors, but not whether a Linux tray host exists.
/// Check the `StatusNotifierWatcher` before hiding, including after a shell restart.
#[cfg(all(unix, not(target_vendor = "apple")))]
pub(crate) async fn available() -> bool {
    use zbus::{Connection, fdo::DBusProxy, names::BusName};

    let check = async {
        let connection = Connection::session().await?;
        let proxy = DBusProxy::new(&connection).await?;
        let name = BusName::try_from("org.kde.StatusNotifierWatcher")?;
        Ok::<bool, zbus::Error>(proxy.name_has_owner(name).await?)
    };
    check.await.unwrap_or(false)
}

#[cfg(not(all(unix, not(target_vendor = "apple"))))]
pub(crate) fn available() -> Ready<bool> {
    ready(true)
}
