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
fn desktop_exec_launches_paths_with_field_codes_and_shell_metacharacters() -> TestResult {
    let temp = tempfile::tempdir()?;
    let artifact = temp.path().join("workshop.desktop");
    for path in ["/tmp/a%u$`\"\\workshop", "/opt/\\\\x/$HOME/%%/workshop"] {
        set_at(&artifact, Path::new(path), true)?;
        let entry = fs::read_to_string(&artifact)?;
        let exec = entry
            .lines()
            .find_map(|line| line.strip_prefix("Exec="))
            .ok_or("missing Exec key")?;
        assert_eq!(exec_path(exec)?, path, "{exec}");
    }
    assert!(
        set_at(&artifact, Path::new("/tmp/work\tshop"), true).is_err(),
        "control characters are refused, never written raw"
    );
    Ok(())
}

/// Independent model of the spec for one quoted argument: string unescaping (`\\`), then
/// Exec quoting and `%%`. Anything the generator must never emit is an error.
#[cfg(target_os = "linux")]
fn exec_path(value: &str) -> Result<String, Box<dyn Error>> {
    let mut string = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.next() != Some('\\') {
            return Err("unexpected string escape".into());
        }
        string.push(character);
    }
    let inner = string
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("not one quoted argument")?;
    let mut path = String::new();
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some(escaped @ ('"' | '`' | '$' | '\\')) => path.push(escaped),
                other => return Err(format!("invalid Exec escape {other:?}").into()),
            },
            '"' | '`' | '$' => return Err(format!("unescaped {character} in quotes").into()),
            '%' => {
                if characters.next() != Some('%') {
                    return Err("field code".into());
                }
                path.push('%');
            },
            other => path.push(other),
        }
    }
    Ok(path)
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
