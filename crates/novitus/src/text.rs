//! Control-free Windows-1250 fields; no transliteration or truncation.
//! Character counts prove codec support, not the printer's font repertoire.

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
