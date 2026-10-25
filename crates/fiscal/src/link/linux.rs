//! Passive unique_number-core UART metadata; never opens the device node.

use std::{fs, path::Path};

pub(super) fn uart_type(port: &str) -> Option<u32> {
    let name = Path::new(port).file_name()?;
    let path = Path::new("/sys/class/tty").join(name).join("type");
    fs::read_to_string(path).ok()?.trim().parse().ok()
}
