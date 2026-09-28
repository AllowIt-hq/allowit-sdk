//! Geometric purchase bands. All amounts use exact six-decimal policy units.
use crate::{Context, Decision};

pub fn purchase_band(amount: u64, maximum: u64) -> Option<usize> {
    if amount == 0 || maximum == 0 || amount > maximum {
        return None;
    }
    let (mut ceiling, mut band) = (maximum, 0);
    while amount <= ceiling / 2 {
        ceiling /= 2;
        band += 1;
    }
    Some(band)
}

// Matches the public evaluator's small, fixed-size decision record.
#[allow(clippy::result_large_err)]
pub fn check_tiers(ctx: &Context, maximum: u64, first_count: u64) -> Result<(), Decision> {
    if maximum > 1_000_000_000_000 || first_count == 0 || first_count > 1_000_000 {
        return Err(Decision::fail(
            "INVALID_POLICY",
            "Invalid purchase-tier limits.",
        ));
    }
    let band = purchase_band(ctx.amount_units, maximum).ok_or_else(|| {
        Decision::fail(
            "PURCHASE_CAP_EXCEEDED",
            "The purchase exceeds its price ceiling.",
        )
    })?;
    let counts = ctx
        .purchase_counts
        .as_ref()
        .filter(|v| v.len() == 40)
        .ok_or_else(|| {
            Decision::fail(
                "LEDGER_REQUIRED",
                "Authoritative purchase counts are required.",
            )
        })?;
    let limit = first_count
        .checked_shl(band as u32)
        .ok_or_else(|| Decision::fail("INVALID_POLICY", "The purchase-tier count is too large."))?;
    if counts[band] >= limit {
        return Err(Decision::fail(
            "PURCHASE_TIER_EXCEEDED",
            "The purchase limit for this price band has been reached.",
        ));
    }
    Ok(())
}
