//! B:128-139,156-254,342-390: bounded frames with CRC-16/ARC.
use fiscal::error::Error;
use std::str;

pub const MAX_FRAME: usize = 1024;
pub const STX: u8 = 2;
pub const ETX: u8 = 3;
pub const FS: u8 = 0x1C;

pub fn crc(bytes: &[u8]) -> u16 {
    let mut value = 0_u16;
    for byte in bytes {
        value ^= u16::from(*byte);
        for _ in 0..8 {
            value = (value >> 1) ^ if value & 1 == 1 { 0xA001 } else { 0 };
        }
    }
    value
}

pub fn encode(header: &str, data: &[u8]) -> Result<Vec<u8>, Error> {
    if header.is_empty()
        || !header.is_ascii()
        || header.bytes().any(|b| matches!(b, STX | ETX | FS))
        || data.iter().any(|b| matches!(*b, STX | ETX))
        || header.len() + data.len() + 7 > MAX_FRAME
    {
        return Err(corrupt());
    }
    let mut raw = Vec::with_capacity(header.len() + data.len() + 7);
    raw.push(STX);
    raw.extend_from_slice(header.as_bytes());
    raw.push(FS);
    raw.extend_from_slice(data);
    let checksum = crc(raw.get(1..).ok_or_else(corrupt)?);
    for shift in [12, 8, 4, 0] {
        let nibble = u8::try_from((checksum >> shift) & 0xF).map_err(|_overflow| corrupt())?;
        raw.push(if nibble < 10 {
            b'0' + nibble
        } else {
            b'A' + nibble - 10
        });
    }
    raw.push(ETX);
    Ok(raw)
}

pub fn decode(raw: &[u8]) -> Result<(&str, &[u8]), Error> {
    if !(8..=MAX_FRAME).contains(&raw.len())
        || raw.first() != Some(&STX)
        || raw.last() != Some(&ETX)
    {
        return Err(corrupt());
    }
    let end = raw.len() - 5;
    let content = raw.get(1..end).ok_or_else(corrupt)?;
    if content.iter().any(|b| matches!(*b, STX | ETX)) {
        return Err(corrupt());
    }
    let hex = str::from_utf8(raw.get(end..raw.len() - 1).ok_or_else(corrupt)?)
        .map_err(|_encoding| corrupt())?;
    if hex.len() != 4
        || !hex.bytes().all(|b| b.is_ascii_hexdigit())
        || u16::from_str_radix(hex, 16).map_err(|_number| corrupt())? != crc(content)
    {
        return Err(corrupt());
    }
    let separator = content.iter().position(|b| *b == FS).ok_or_else(corrupt)?;
    let header = str::from_utf8(content.get(..separator).ok_or_else(corrupt)?)
        .map_err(|_encoding| corrupt())?;
    if header.is_empty() || !header.is_ascii() {
        return Err(corrupt());
    }
    Ok((header, content.get(separator + 1..).ok_or_else(corrupt)?))
}

pub(crate) fn corrupt() -> Error {
    Error::Protocol("Nieprawidłowa ramka ElzabSTX.".to_owned())
}
#[cfg(test)]
mod tests;
