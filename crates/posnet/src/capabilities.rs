//! Temo Online limits and verbatim text repertoire.
//!
//! Spec: POSNET POT-I-DEV-37 v5978 (Online MR2021).

use fiscal::device::Capabilities;

use crate::text;

/// Undetectable device limit: `trline` `na`, p. 295.
const MAX_ITEM_NAME: usize = 80;
/// Transaction limit: `trinit`, p. 275.
const MAX_LINES: usize = 500;
pub(crate) const MAX_BUYER_NIP: usize = 30;
/// Company footer limit: `trftrln` `na`, used when the header lacks the company name.
const MAX_COMPANY_NAME: usize = 40;
const MAX_QUANTITY_DECIMALS: u8 = 8;

/// Posnet reserves some Windows-1250 bytes for table-drawing glyphs (p. 14).
/// Holuto excludes them before queueing because the driver never rewrites text.
fn printable_characters() -> String {
    (0x20..=0xFF_u8)
        .filter_map(|byte| text::decode(&[byte])?.chars().next())
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    character,
                    '\u{A0}' | '\u{A4}' | '\u{A8}' | '\u{AD}' | '\u{2DB}' | '\u{B4}' | '\u{B8}'
                )
        })
        .collect()
}

pub fn capabilities() -> Capabilities {
    Capabilities {
        max_item_name: MAX_ITEM_NAME,
        max_lines: MAX_LINES,
        max_company_name: MAX_COMPANY_NAME,
        max_buyer_nip: MAX_BUYER_NIP,
        max_quantity_decimals: MAX_QUANTITY_DECIMALS,
        allowed_characters: printable_characters(),
        advance_deductions: true,
        zero_total_receipts: true,
        company_footer: true,
    }
}

#[cfg(test)]
mod tests;
