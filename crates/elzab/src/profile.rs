//! Candidate XTR-B name profiles: numeric IDs/firmware matrix remain unverified.
//!
//! elzab-oss.md §5: only Cube's literal XTR name is vendor documented; the two
//! uppercase ONLINE names are explicit candidate spellings, not substring matches.
//! Actual D firmware is published unchanged, never invented from model numbers.
use crate::text;
use fiscal::device::Capabilities;

#[derive(Clone, Debug)]
pub struct Profile {
    pub capabilities: Capabilities,
    /// ONLINE line-by-line open variant (P:3583-3620), never sent to legacy ZETA.
    pub open: &'static str,
}

pub fn lookup(model: &str) -> Option<Profile> {
    match model {
        "ELZAB ZETA ONLINE" | "ELZAB MERA ONLINE" | "ELZAB CUBE online" => {},
        _ => return None,
    }
    Some(Profile {
        capabilities: Capabilities {
            // Current ONLINE model manuals Z:1971-1989, M:1867-1875, Q:1833-1858.
            max_item_name: 64,
            // Host safety ceiling, not a claim about unverified device buffer size
            // (elzab-oss.md §6). A device buffer refusal is never replayed.
            max_lines: 255,
            // Conservative narrow main-line width (P:378-392); LAL is disabled.
            max_company_name: 42,
            max_buyer_nip: 42, // P:2381-2386: line_max_len, conservative narrow width.
            max_quantity_decimals: 5, // P:420-421,781-785.
            allowed_characters: text::printable_cp1250(),
            advance_deductions: true,   // P:4131-4166, ONLINE LDS extension.
            zero_total_receipts: false, // Error1008; elzab-oss.md §1 unresolved.
            company_footer: false,      // Native LAL ordering unqualified, OSS §2.
        },
        // ONLINE line-by-line variant; P:3583-3620. Never match legacy ZETA.
        open: "LOR\x1dC0",
    })
}
