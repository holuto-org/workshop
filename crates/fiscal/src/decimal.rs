//! Exact decimals in `10^-scale` units.

use std::iter;

/// Parses ASCII digits with an optional decimal point and at most `scale` fraction digits.
/// Returns `None` for invalid syntax or overflow.
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

/// Formats scaled units without trailing fraction zeros.
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
