//! Text at the device boundary: the printer speaks Windows-1250 (its required code page), every
//! field is tab-separated, and an item name holds at most 80 characters.

use crate::Error;

/// Temo Online capabilities the protocol cannot report, from Posnet POT-I-DEV-37 v5978
/// (Online MR2021): an item name (`trline` `na`, p. 295) holds "Do 80 znaków".
pub const MAX_NAME_LENGTH: usize = 80;
/// A started transaction (`trinit`, p. 275) takes "do 500 pozycji sprzedaży".
pub const MAX_LINES: usize = 500;
pub const MAX_BUYER_NIP_LENGTH: usize = 30;
/// The footer line (`trftrln` `na`) carrying the company name when the printer header lacks it.
pub const MAX_COMPANY_NAME_LENGTH: usize = 40;

/// Windows-1250 bytes 0x80..=0xFF; the five bytes the code page leaves unassigned are `None`.
const HIGH_HALF: [Option<char>; 128] = [
    Some('\u{20AC}'),
    None,
    Some('\u{201A}'),
    None,
    Some('\u{201E}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    None,
    Some('\u{2030}'),
    Some('\u{0160}'),
    Some('\u{2039}'),
    Some('\u{015A}'),
    Some('\u{0164}'),
    Some('\u{017D}'),
    Some('\u{0179}'),
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201C}'),
    Some('\u{201D}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    None,
    Some('\u{2122}'),
    Some('\u{0161}'),
    Some('\u{203A}'),
    Some('\u{015B}'),
    Some('\u{0165}'),
    Some('\u{017E}'),
    Some('\u{017A}'),
    Some('\u{00A0}'),
    Some('\u{02C7}'),
    Some('\u{02D8}'),
    Some('\u{0141}'),
    Some('\u{00A4}'),
    Some('\u{0104}'),
    Some('\u{00A6}'),
    Some('\u{00A7}'),
    Some('\u{00A8}'),
    Some('\u{00A9}'),
    Some('\u{015E}'),
    Some('\u{00AB}'),
    Some('\u{00AC}'),
    Some('\u{00AD}'),
    Some('\u{00AE}'),
    Some('\u{017B}'),
    Some('\u{00B0}'),
    Some('\u{00B1}'),
    Some('\u{02DB}'),
    Some('\u{0142}'),
    Some('\u{00B4}'),
    Some('\u{00B5}'),
    Some('\u{00B6}'),
    Some('\u{00B7}'),
    Some('\u{00B8}'),
    Some('\u{0105}'),
    Some('\u{015F}'),
    Some('\u{00BB}'),
    Some('\u{013D}'),
    Some('\u{02DD}'),
    Some('\u{013E}'),
    Some('\u{017C}'),
    Some('\u{0154}'),
    Some('\u{00C1}'),
    Some('\u{00C2}'),
    Some('\u{0102}'),
    Some('\u{00C4}'),
    Some('\u{0139}'),
    Some('\u{0106}'),
    Some('\u{00C7}'),
    Some('\u{010C}'),
    Some('\u{00C9}'),
    Some('\u{0118}'),
    Some('\u{00CB}'),
    Some('\u{011A}'),
    Some('\u{00CD}'),
    Some('\u{00CE}'),
    Some('\u{010E}'),
    Some('\u{0110}'),
    Some('\u{0143}'),
    Some('\u{0147}'),
    Some('\u{00D3}'),
    Some('\u{00D4}'),
    Some('\u{0150}'),
    Some('\u{00D6}'),
    Some('\u{00D7}'),
    Some('\u{0158}'),
    Some('\u{016E}'),
    Some('\u{00DA}'),
    Some('\u{0170}'),
    Some('\u{00DC}'),
    Some('\u{00DD}'),
    Some('\u{0162}'),
    Some('\u{00DF}'),
    Some('\u{0155}'),
    Some('\u{00E1}'),
    Some('\u{00E2}'),
    Some('\u{0103}'),
    Some('\u{00E4}'),
    Some('\u{013A}'),
    Some('\u{0107}'),
    Some('\u{00E7}'),
    Some('\u{010D}'),
    Some('\u{00E9}'),
    Some('\u{0119}'),
    Some('\u{00EB}'),
    Some('\u{011B}'),
    Some('\u{00ED}'),
    Some('\u{00EE}'),
    Some('\u{010F}'),
    Some('\u{0111}'),
    Some('\u{0144}'),
    Some('\u{0148}'),
    Some('\u{00F3}'),
    Some('\u{00F4}'),
    Some('\u{0151}'),
    Some('\u{00F6}'),
    Some('\u{00F7}'),
    Some('\u{0159}'),
    Some('\u{016F}'),
    Some('\u{00FA}'),
    Some('\u{0171}'),
    Some('\u{00FC}'),
    Some('\u{00FD}'),
    Some('\u{0163}'),
    Some('\u{02D9}'),
];

/// The Windows-1250 byte for `character`, if the code page has one.
fn encode_char(character: char) -> Option<u8> {
    if character.is_ascii() {
        return u8::try_from(character).ok();
    }
    let index = HIGH_HALF
        .iter()
        .position(|entry| *entry == Some(character))?;
    u8::try_from(index).ok().map(|index| index | 0x80)
}

fn decode_byte(byte: u8) -> Option<char> {
    if byte.is_ascii() {
        Some(char::from(byte))
    } else {
        HIGH_HALF.get(usize::from(byte & 0x7F)).copied().flatten()
    }
}

/// `text` as Windows-1250 bytes, or `None` when a character has no byte.
pub(crate) fn encode(text: &str) -> Option<Vec<u8>> {
    text.chars().map(encode_char).collect()
}

/// Device bytes as text, or `None` for an unassigned byte (a corrupt frame).
pub(crate) fn decode(bytes: &[u8]) -> Option<String> {
    bytes.iter().map(|byte| decode_byte(*byte)).collect()
}

/// Whether `character` prints as itself: it has a Windows-1250 byte and is visible text. That
/// excludes the control bytes, the no-break space and the soft hyphen.
fn is_printable(character: char) -> bool {
    !character.is_control()
        && !matches!(character, '\u{A0}' | '\u{AD}')
        && encode_char(character).is_some()
}

/// Every character receipt text may hold.
///
/// The Windows-1250 bytes 0x20..=0xFF that have a character, except DEL, the no-break space and
/// the soft hyphen. The agent reports this set so Holuto rejects unprintable text before
/// queueing; the driver never rewrites text.
pub fn printable_characters() -> String {
    (0x20..=0xFF_u8)
        .filter_map(decode_byte)
        .filter(|character| is_printable(*character))
        .collect()
}

/// Whether every character of `text` is in [`printable_characters`].
pub(crate) fn is_printable_text(text: &str) -> bool {
    text.chars().all(is_printable)
}

/// Rejects text a frame cannot carry: control characters (a tab separates fields) or characters
/// without a Windows-1250 byte.
pub(crate) fn check_printable(text: &str) -> Result<(), Error> {
    if text.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Tekst nie może zawierać znaków sterujących.".to_owned(),
        ));
    }
    if !text
        .chars()
        .all(|character| encode_char(character).is_some())
    {
        return Err(Error::Invalid(
            "Drukarka nie obsługuje niektórych znaków w tekście.".to_owned(),
        ));
    }
    Ok(())
}

/// Header text as printed: `&&` is a literal `&`, any other `&x` is a formatting mark.
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
