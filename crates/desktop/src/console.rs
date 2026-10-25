//! Keep Explorer launches windowless; attach CLI launches to the parent console.
//! Preserve inherited pipes and redirected handles.

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    },
};

#[cfg(windows)]
pub(crate) fn prepare(cli: bool) {
    if cli {
        attach_parent();
    }
}

#[cfg(not(windows))]
pub(crate) const fn prepare(_cli: bool) {}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Win32 console attachment has no safe std equivalent; preserve inherited handles"
)]
fn attach_parent() {
    // SAFETY: GetStdHandle takes only fixed Win32 constants and returns borrowed handles.
    let (output, error) = unsafe {
        (
            GetStdHandle(STD_OUTPUT_HANDLE),
            GetStdHandle(STD_ERROR_HANDLE),
        )
    };
    let valid = |handle: HANDLE| !handle.is_null() && handle != INVALID_HANDLE_VALUE;
    if !valid(output) && !valid(error) {
        // SAFETY: fixed parent-process sentinel; no pointers or owned handles.
        // A missing parent console is normal; never allocate one.
        let _attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(all(test, windows))]
mod tests;
