//! Strict model/firmware allowlist, not a marketing-name or version-range fallback.
//!
//! N:L6216-L6231 supplies literal firmware spellings; `210` is not guessed to
//! mean `2.10`. Native identity spellings still require hardware qualification
//! (novitus-oss.md §6). These are explicit candidate profiles, not a parity claim.

use fiscal::device::Capabilities;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub capabilities: Capabilities,
    /// Conservative width independent of paper/font configuration.
    pub info_line_width: usize,
}

pub fn lookup(model: &str, firmware: &str) -> Option<Profile> {
    // N:L6216-L6231: no inferred aliases or newer-version compatibility.
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
            max_lines: 255, // N:L6072,L6180-L6181; no online index wrapping.
            max_company_name: info_line_width,
            max_buyer_nip: 16, // N:L7785-L7813.
            max_quantity_decimals: 8,
            // N:L538-L546,L599-L602 establishes printable ASCII only. The rest
            // of CP1250 is not advertised without verified firmware font evidence.
            allowed_characters: (' '..='~').collect(),
            advance_deductions: false,
            zero_total_receipts: false,
            company_footer: false,
        },
        // HD II is absent from the 24$z width table: use the documented $y
        // width 40 (N:L6935-L6939), not HD's 64. Point's narrow width is 44;
        // Bono/Deon 42 (N:L7682-L7699). Footer remains unqualified regardless.
        info_line_width,
    })
}
