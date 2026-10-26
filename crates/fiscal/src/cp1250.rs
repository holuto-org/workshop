//! Windows-1250 encoding shared by printer protocols; unassigned bytes are rejected.

/// Windows-1250 bytes 0x80–0xFF; `None` marks the five unassigned bytes.
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

pub(crate) fn encode_char(character: char) -> Option<u8> {
    if character.is_ascii() {
        return u8::try_from(character).ok();
    }
    let index = HIGH_HALF
        .iter()
        .position(|entry| *entry == Some(character))?;
    u8::try_from(index).ok().map(|index| index | 0x80)
}

/// One Windows-1250 byte; the five unassigned byte values return `None`.
pub fn decode_byte(byte: u8) -> Option<char> {
    if byte.is_ascii() {
        Some(char::from(byte))
    } else {
        HIGH_HALF.get(usize::from(byte & 0x7F)).copied().flatten()
    }
}

pub fn encode(text: &str) -> Option<Vec<u8>> {
    text.chars().map(encode_char).collect()
}

pub fn decode(bytes: &[u8]) -> Option<String> {
    bytes.iter().map(|byte| decode_byte(*byte)).collect()
}

#[cfg(test)]
mod tests;
