//! Posnet header formatting and shared Windows-1250 encoding.

use fiscal::cp1250;

pub(crate) fn encode(text: &str) -> Option<Vec<u8>> {
    cp1250::encode(text)
}

pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    cp1250::decode(bytes)
}

/// Header escapes: `&&` prints `&`; other `&x` pairs are formatting marks.
pub(crate) fn header_text(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        if character != '&' {
            text.push(character);
        } else if characters.next() == Some('&') {
            text.push('&');
        }
    }
    text
}

#[cfg(test)]
mod tests;
