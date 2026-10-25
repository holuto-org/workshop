//! ESC P / ESC backslash frames and FF-initialized XOR, N:L538-L576.
//! `encode`/`decode` require checksums; `query`/`unframe` are only for schemas
//! explicitly omitting them (N:L583-L587,L660-L668,L1236-L1248).

use fiscal::error::Error;

pub const START: [u8; 2] = [0x1B, b'P'];
pub const END: [u8; 2] = [0x1B, b'\\'];
pub const MAX_FRAME: usize = 16_384;
const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub fn encode(body: &[u8]) -> Vec<u8> {
    let control = body.iter().fold(0xFF, |control, byte| control ^ byte);
    let mut raw = Vec::with_capacity(body.len() + 6);
    raw.extend_from_slice(&START);
    raw.extend_from_slice(body);
    raw.push(hex_digit(control >> 4));
    raw.push(hex_digit(control & 15));
    raw.extend_from_slice(&END);
    raw
}

const fn hex_digit(nibble: u8) -> u8 {
    if nibble < 10 {
        b'0' + nibble
    } else {
        b'A' + nibble - 10
    }
}

pub fn decode(raw: &[u8]) -> Result<&[u8], Error> {
    let payload = unframe(raw)?;
    let (body, checksum) = payload
        .split_at_checked(payload.len().saturating_sub(2))
        .ok_or_else(corrupt)?;
    if body.is_empty() || checksum.len() != 2 {
        return Err(corrupt());
    }
    let expected = checksum
        .iter()
        .try_fold(0_u8, |value, byte| {
            HEX.iter()
                .position(|digit| digit == byte)
                .and_then(|digit| u8::try_from(digit).ok())
                .map(|digit| (value << 4) | digit)
        })
        .ok_or_else(corrupt)?;
    if body.iter().fold(0xFF, |control, byte| control ^ byte) != expected {
        return Err(corrupt());
    }
    Ok(body)
}

/// Unchecksummed envelope, only for the command-specific exceptions in N.
pub fn query(body: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(body.len() + 4);
    raw.extend_from_slice(&START);
    raw.extend_from_slice(body);
    raw.extend_from_slice(&END);
    raw
}

pub fn unframe(raw: &[u8]) -> Result<&[u8], Error> {
    raw.strip_prefix(&START)
        .and_then(|body| body.strip_suffix(&END))
        .filter(|body| !body.is_empty() && raw.len() <= MAX_FRAME && !body.contains(&0x1B))
        .ok_or_else(corrupt)
}

pub(crate) fn corrupt() -> Error {
    Error::Protocol("Uszkodzona ramka Novitus (suma kontrolna lub format).".to_owned())
}

#[cfg(test)]
mod tests;
