//! Candidate model/firmware profiles, not hardware qualification.
//! Firmware spellings are literal: no aliases or version-range fallback.

use fiscal::{cp1250, device::Capabilities};

pub fn lookup(model: &str, firmware: &str) -> Option<Capabilities> {
    // `max_company_name` is the conservative info-line width, independent of paper/font
    // configuration. HD II has no 24$z width entry: use $y's 40 columns, not HD's 64.
    let (max_item_name, max_company_name) = match (model, firmware) {
        ("NOVITUS HD II ONLINE", "1.40" | "2.20" | "3.50") => (60, 40),
        ("NOVITUS DEON ONLINE", "1.40") | ("NOVITUS BONO ONLINE", "1.10") => (40, 42),
        ("NOVITUS DEON ONLINE", "220" | "310") | ("NOVITUS BONO ONLINE", "210" | "300") => (60, 42),
        ("NOVITUS POINT", "1.0") => (60, 44),
        _ => return None,
    };
    Some(Capabilities {
        max_item_name,
        max_lines: 255, // No online index wrapping.
        max_company_name,
        max_buyer_nip: 16,
        max_quantity_decimals: 8,
        // ASCII only until 52$i confirms attr1 bit 0x0200 (full Windows-1250).
        allowed_characters: (' '..='~').collect(),
        advance_deductions: cfg!(workshop_acceptance),
        zero_total_receipts: cfg!(workshop_acceptance),
        // Footer support still requires hardware qualification.
        company_footer: cfg!(workshop_acceptance),
    })
}

/// Repertoire once `52$i` confirms full Windows-1250.
pub(crate) fn full_cp1250_characters() -> String {
    (0x20..=u8::MAX)
        .filter_map(cp1250::decode_byte)
        .filter(|character| !character.is_control())
        .collect()
}
