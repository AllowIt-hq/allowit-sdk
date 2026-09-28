//! Exact decimal literals used by source helpers. No floating point or rounding.
use crate::CompileError;

pub(crate) fn decimal_units(value: &str, decimals: u32) -> Result<u64, CompileError> {
    let invalid = || {
        CompileError::new(
            "INVALID_LITERAL",
            "Use a non-negative decimal string with no rounding, exponent, sign or separators.",
        )
    };
    if value.is_empty() || value.len() > 24 {
        return Err(invalid());
    }
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || (value.contains('.') && fraction.is_empty())
        || fraction.len() > decimals as usize
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }
    let whole = whole.parse::<u64>().map_err(|_| invalid())?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().map_err(|_| invalid())?
    };
    whole
        .checked_mul(10_u64.pow(decimals))
        .and_then(|n| {
            fraction
                .checked_mul(10_u64.pow(decimals - value_fraction_len(value)))
                .and_then(|f| n.checked_add(f))
        })
        .ok_or_else(invalid)
}

fn value_fraction_len(value: &str) -> u32 {
    value
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len() as u32)
}

pub(crate) fn percentage_bps(value: &str) -> Result<u64, CompileError> {
    let bps = decimal_units(value, 2)?;
    if bps > 10_000 {
        return Err(CompileError::new(
            "INVALID_LITERAL",
            "Percentages must be between 0 and 100, with at most two decimal places.",
        ));
    }
    Ok(bps)
}
