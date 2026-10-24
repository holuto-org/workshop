//! Plain decimal text as an exact count of `10^-scale` units: `"12.50"` at scale 2 is 1250.

use std::iter;

/// The units `text` holds: ASCII digits, optionally a `.` and at most `scale` fraction digits.
/// `None` for a sign, exponent, separator, blank, missing digit or a value above `u64::MAX`.
pub fn parse(text: &str, scale: usize) -> Option<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() || text.ends_with('.') {
        return None;
    }
    let padding = scale.checked_sub(fraction.len())?;
    whole
        .chars()
        .chain(fraction.chars())
        .chain(iter::repeat_n('0', padding))
        .try_fold(0_u64, |units, digit| {
            units
                .checked_mul(10)?
                .checked_add(u64::from(digit.to_digit(10)?))
        })
}

/// `units` as plain text without trailing fraction zeros: 150 000 000 at scale 8 is `1.5`.
pub fn format(units: u64, scale: usize) -> String {
    let digits = format!("{units:0>width$}", width = scale + 1);
    let (whole, fraction) = digits.split_at(digits.len() - scale);
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    }
}

#[cfg(test)]
mod tests;
