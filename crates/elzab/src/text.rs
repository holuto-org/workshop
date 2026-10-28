//! Configured CP1250 only, never replacement or escaping.
use fiscal::{cp1250, error::Error};

pub(crate) fn encode_body(command: &str, fields: &str) -> Result<Vec<u8>, Error> {
    let body = if fields.is_empty() {
        command.to_owned()
    } else {
        format!("{command}\x1d{fields}")
    };
    // Separators belong to the command grammar, never caller-supplied text.
    if body
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\u{1d}' | '\u{1e}' | '\u{1f}'))
    {
        return Err(Error::Invalid(i18n::Message::new(
            "drivers-elzab-invalid-character",
        )));
    }
    cp1250::encode(&body)
        .ok_or_else(|| Error::Invalid(i18n::Message::new("drivers-cp1250-command")))
}

pub fn printable_cp1250() -> String {
    (0x20..=u8::MAX)
        .filter_map(cp1250::decode_byte)
        .filter(|c| !c.is_control())
        .collect()
}

#[cfg(test)]
mod tests;
