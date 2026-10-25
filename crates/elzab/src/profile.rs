//! Candidate XTR-B name profiles: numeric IDs/firmware matrix remain unverified.
//!
//! Only Cube's literal XTR name is vendor documented; the two
//! uppercase ONLINE names are explicit candidate spellings, not substring matches.
//! Actual D firmware is published unchanged, never invented from model numbers.
use crate::text;
use fiscal::device::Capabilities;

#[derive(Clone, Debug)]
pub struct Profile {
    pub capabilities: Capabilities,
    /// ONLINE line-by-line open variant, never sent to legacy ZETA.
    pub open: &'static str,
}

pub fn lookup(model: &str) -> Option<Profile> {
    match model {
        "ELZAB ZETA ONLINE" | "ELZAB MERA ONLINE" | "ELZAB CUBE online" => {},
        _ => return None,
    }
    Some(Profile {
        capabilities: Capabilities {
            // Current ONLINE model manuals.
            max_item_name: 64,
            // Host ceiling, not a verified device limit; a refusal is never replayed.
            max_lines: 255,
            // Conservative narrow line width; LAL is disabled.
            max_company_name: 42,
            max_buyer_nip: 42,
            max_quantity_decimals: 5,
            allowed_characters: text::printable_cp1250(),
            advance_deductions: true, // ONLINE LDS extension.
            zero_total_receipts: cfg!(workshop_acceptance), // Error1008, unqualified.
            company_footer: cfg!(workshop_acceptance), // Native LAL order unqualified.
        },
        open: "LOR\x1dC0",
    })
}
