//! Strict model/firmware allowlist, not a marketing-name or version-range fallback.
//!
//! The protocol supplies literal firmware spellings; `210` is not guessed to
//! mean `2.10`. Native identity spellings still require hardware qualification.
//! These are explicit candidate profiles, not a parity claim.

use fiscal::{cp1250, device::Capabilities};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub capabilities: Capabilities,
    /// Conservative width independent of paper/font configuration.
    pub info_line_width: usize,
}

pub fn lookup(model: &str, firmware: &str) -> Option<Profile> {
    // Literal firmware spellings: no inferred aliases or newer-version compatibility.
    let (max_item_name, info_line_width) = match (model, firmware) {
        ("NOVITUS HD II ONLINE", "1.40" | "2.20" | "3.50") => (60, 40),
        ("NOVITUS DEON ONLINE", "1.40") | ("NOVITUS BONO ONLINE", "1.10") => (40, 42),
        ("NOVITUS DEON ONLINE", "220" | "310") | ("NOVITUS BONO ONLINE", "210" | "300") => (60, 42),
        ("NOVITUS POINT", "1.0") => (60, 44),
        _ => return None,
    };
    Some(Profile {
        capabilities: Capabilities {
            max_item_name,
            max_lines: 255, // No online index wrapping.
            max_company_name: info_line_width,
            max_buyer_nip: 16,
            max_quantity_decimals: 8,
            // Until read-only 52$i proves attr1 0x0200, advertise only ASCII.
            allowed_characters: (' '..='~').collect(),
            advance_deductions: cfg!(workshop_acceptance),
            zero_total_receipts: cfg!(workshop_acceptance),
            company_footer: cfg!(workshop_acceptance),
        },
        // HD II is absent from the 24$z width table: use the documented $y
        // width 40, not HD's 64. Point's narrow width is 44; Bono/Deon 42.
        // Footer remains unqualified regardless.
        info_line_width,
    })
}

impl Profile {
    /// The protocol explicitly promises every Windows-1250 glyph for this attribute.
    pub(crate) fn enable_full_cp1250(&mut self) {
        self.capabilities.allowed_characters = (0x20..=u8::MAX)
            .filter_map(cp1250::decode_byte)
            .filter(|character| !character.is_control())
            .collect();
    }
}
