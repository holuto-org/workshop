//! Posnet Temo Online validation limits, preserved during the fiscal extraction.
//! Driver capabilities replace these vendor-specific rules in Task 3.

use crate::{
    cp1250::{decode_byte, encode_char},
    error::Error,
};

/// Undetectable device limit: `trline` `na`, Posnet POT-I-DEV-37 v5978 (Online MR2021), p. 295.
pub const MAX_NAME_LENGTH: usize = 80;
/// Transaction limit: `trinit`, same spec, p. 275.
pub const MAX_LINES: usize = 500;
pub const MAX_BUYER_NIP_LENGTH: usize = 30;
/// Company footer limit: `trftrln` `na`, used when the header lacks the company name.
pub const MAX_COMPANY_NAME_LENGTH: usize = 40;

fn is_printable(character: char) -> bool {
    // POT-I-DEV-37 v5978 p. 14 reserves these bytes for table-drawing glyphs.
    !character.is_control()
        && !matches!(
            character,
            '\u{A0}' | '\u{A4}' | '\u{A8}' | '\u{AD}' | '\u{2DB}' | '\u{B4}' | '\u{B8}'
        )
        && encode_char(character).is_some()
}

/// Advertised to Holuto for validation before queueing; the driver never rewrites text.
/// Assigned Windows-1250 bytes 0x20–0xFF, excluding DEL and the table-drawing glyphs (spec p. 14).
pub fn printable_characters() -> String {
    (0x20..=0xFF_u8)
        .filter_map(decode_byte)
        .filter(|character| is_printable(*character))
        .collect()
}

pub(crate) fn is_printable_text(text: &str) -> bool {
    text.chars().all(is_printable)
}

/// Controls would corrupt framing; all other characters need a Windows-1250 byte.
pub fn check_printable(text: &str) -> Result<(), Error> {
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

#[cfg(test)]
mod tests;
