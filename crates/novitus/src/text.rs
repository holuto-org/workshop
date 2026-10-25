//! Windows-1250 text fields, with no control bytes, N:L538-L562.
//!
//! Character counting establishes codec representability, not a verified device
//! font repertoire (novitus-oss.md §7); no transliteration or truncation.

use fiscal::cp1250;

pub fn encode(text: &str) -> Option<Vec<u8>> {
    if text.chars().any(char::is_control) {
        return None;
    }
    cp1250::encode(text)
}

pub fn decode(bytes: &[u8]) -> Option<String> {
    let text = cp1250::decode(bytes)?;
    (!text.chars().any(char::is_control)).then_some(text)
}

pub fn printable_characters(text: &str) -> Option<usize> {
    encode(text).map(|bytes| bytes.len())
}

#[cfg(test)]
mod tests;
