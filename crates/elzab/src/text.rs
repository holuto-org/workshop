//! P:569-573,3229-3236: configured CP1250 only, never replacement or escaping.
use fiscal::{cp1250, error::Error};

pub fn encode(text: &str) -> Result<Vec<u8>, Error> {
    if text.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Tekst zawiera znaki sterujące ElzabSTX.".to_owned(),
        ));
    }
    cp1250::encode(text).ok_or_else(|| Error::Invalid("Tekst nie mieści się w CP1250.".to_owned()))
}

pub(crate) fn encode_body(body: &str) -> Result<Vec<u8>, Error> {
    // Separators belong to the command grammar, never caller-supplied text.
    if body
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\u{1d}' | '\u{1e}' | '\u{1f}'))
    {
        return Err(Error::Invalid(
            "Niedozwolony znak w poleceniu ElzabSTX.".to_owned(),
        ));
    }
    cp1250::encode(body)
        .ok_or_else(|| Error::Invalid("Polecenie nie mieści się w CP1250.".to_owned()))
}

pub fn printable_cp1250() -> String {
    (0x20..=u8::MAX)
        .filter_map(cp1250::decode_byte)
        .filter(|c| !c.is_control())
        .collect()
}

#[cfg(test)]
mod tests;
