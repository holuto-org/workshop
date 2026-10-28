//! Keep Explorer launches windowless; attach CLI launches to the parent console.
//! Preserve inherited pipes and redirected handles.

#[cfg(windows)]
use std::{io, os::windows::io::AsRawHandle as _};

#[cfg(windows)]
pub(crate) fn prepare(cli: bool) {
    // std reports a missing standard handle as null and looks the handles up again on every
    // write, so output written after attaching reaches the parent console.
    if cli && io::stdout().as_raw_handle().is_null() && io::stderr().as_raw_handle().is_null() {
        // A missing parent console is normal; never allocate one.
        let _attached = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub(crate) const fn prepare(_cli: bool) {}

/// <https://learn.microsoft.com/en-us/windows/console/attachconsole>
#[cfg(windows)]
const ATTACH_PARENT_PROCESS: u32 = u32::MAX;

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "std has no console attachment; one declaration of the documented signature"
)]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// `BOOL AttachConsole(DWORD dwProcessId)`: no pointers, so every argument is sound.
    safe fn AttachConsole(process_id: u32) -> i32;
}

// Separate attributes: clippy recognizes a test module only by a plain `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod tests;
