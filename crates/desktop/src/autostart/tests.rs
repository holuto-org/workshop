#[cfg(not(windows))]
use std::fs;
use std::{error::Error, path::Path};

#[cfg(not(windows))]
use super::set_at;

type TestResult = Result<(), Box<dyn Error>>;

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn linux_login_entry_launches_only_the_exact_executable_and_is_removable() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp.path().join("autostart/workshop.desktop");
    set_at(
        &artifact,
        Path::new("/home/operator/Holuto Workshop/workshop"),
        true,
    )?;
    assert_eq!(
        fs::read_to_string(&artifact)?,
        "[Desktop Entry]\nType=Application\nName=Holuto Workshop\n\
         Exec=\"/home/operator/Holuto Workshop/workshop\"\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    set_at(&artifact, Path::new("/unused"), false)?;
    assert!(!artifact.exists());
    set_at(&artifact, Path::new("/unused"), false)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn desktop_exec_escapes_field_codes_and_shell_metacharacters() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp.path().join("workshop.desktop");
    set_at(&artifact, Path::new("/tmp/a%u$`\"\\workshop"), true)?;
    assert_eq!(
        fs::read_to_string(&artifact)?,
        "[Desktop Entry]\nType=Application\nName=Holuto Workshop\n\
         Exec=\"/tmp/a%%u\\$\\`\\\"\\\\workshop\"\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\n"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove artifacts; setup propagates I/O"
)]
fn macos_login_agent_uses_exact_executable_and_is_removable() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp
        .path()
        .join("Library/LaunchAgents/com.holuto.workshop.plist");
    set_at(
        &artifact,
        Path::new("/Users/operator/Holuto & Workshop.app/Contents/MacOS/workshop"),
        true,
    )?;
    assert_eq!(
        fs::read_to_string(&artifact)?,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict><key>Label</key><string>com.holuto.workshop</string>\
         <key>ProgramArguments</key><array>\
         <string>/Users/operator/Holuto &amp; Workshop.app/Contents/MacOS/workshop</string>\
         </array><key>RunAtLoad</key><true/></dict></plist>\n"
    );
    set_at(&artifact, Path::new("/unused"), false)?;
    assert!(!artifact.exists());
    Ok(())
}

#[cfg(windows)]
#[test]
#[expect(
    unsafe_code,
    clippy::panic_in_result_fn,
    reason = "Read and remove only this test's disposable HKCU key with checked buffers"
)]
fn windows_login_value_contains_only_the_quoted_executable_and_is_removable() -> TestResult {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegDeleteKeyW, RegGetValueW},
    };
    let temp = tempfile::tempdir()?;
    let key = format!(
        "Software\\HolutoWorkshopTests\\{}",
        temp.path()
            .file_name()
            .ok_or("temp directory has no name")?
            .to_string_lossy()
    );
    let executable = Path::new("C:\\Users\\Operator\\Holuto Workshop\\workshop.exe");
    super::registry(&key, executable, true)?;
    let key_wide: Vec<_> = key.encode_utf16().chain(Some(0)).collect();
    let name: Vec<_> = "Holuto Workshop".encode_utf16().chain(Some(0)).collect();
    let expected: Vec<_> = "\"C:\\Users\\Operator\\Holuto Workshop\\workshop.exe\""
        .encode_utf16()
        .chain(Some(0))
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut bytes = vec![0_u8; expected.len()];
    let mut size = u32::try_from(bytes.len())?;
    // SAFETY: live terminated strings, writable byte buffer and byte count match.
    let found = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    assert_eq!(found, ERROR_SUCCESS);
    assert_eq!(bytes, expected);
    super::registry(&key, executable, false)?;
    // SAFETY: same live buffers as above; missing value leaves the buffer untouched.
    let absent = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &raw mut size,
        )
    };
    // SAFETY: removes only the empty key created by this test.
    let removed = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, key_wide.as_ptr()) };
    assert_eq!(absent, ERROR_FILE_NOT_FOUND);
    assert_eq!(removed, ERROR_SUCCESS);
    Ok(())
}
