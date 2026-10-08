//! Primitive source functions. The compiler binds accounting inputs to authenticated fields.
use crate::prelude::{PolicyError, PolicyResult};
use alloc::vec::Vec;
fn denied(code: &str, reason: &str) -> PolicyError {
    PolicyError {
        code: code.into(),
        reason: reason.into(),
    }
}
fn denomination(token: &str, currency: &str, decimals: u64) -> PolicyResult {
    if currency != "USDC" || token != currency || decimals != 6 {
        return Err(denied(
            "TOKEN_MISMATCH",
            "Use the bound six-decimal USDC asset.",
        ));
    }
    Ok(())
}
pub fn set_cap(
    spent_units: u64,
    amount_units: u64,
    token: &str,
    limit_units: u64,
    currency: &str,
    decimals: u64,
) -> PolicyResult {
    denomination(token, currency, decimals)?;
    if limit_units == 0 {
        return Err(denied("INVALID_AMOUNT", "The limit must be positive."));
    }
    if spent_units
        .checked_add(amount_units)
        .is_none_or(|n| n > limit_units)
    {
        return Err(denied(
            "POLICY_CAP_EXCEEDED",
            "Total spending limit exceeded.",
        ));
    }
    Ok(())
}
pub fn cap_per_transaction(
    amount_units: u64,
    token: &str,
    limit_units: u64,
    currency: &str,
    decimals: u64,
) -> PolicyResult {
    denomination(token, currency, decimals)?;
    if limit_units == 0 {
        return Err(denied("INVALID_AMOUNT", "The limit must be positive."));
    }
    if amount_units > limit_units {
        return Err(denied("PURCHASE_CAP_EXCEEDED", "Purchase limit exceeded."));
    }
    Ok(())
}
/// Compare the authenticated purchase amount; authored policies must pass ctx.amount_units.
pub fn amount_at_most(amount_units: u64, amount: &str) -> Result<bool, PolicyError> {
    Ok(amount_units <= crate::prelude::usdc(amount)?)
}
pub fn allow_actions(action: &str, actions: &[&str]) -> PolicyResult {
    if is_one_of(action, actions)? {
        Ok(())
    } else {
        Err(denied("ACTION_NOT_ALLOWED", "Action is not permitted."))
    }
}
pub fn require_merchant(merchant: &str, required: &str) -> PolicyResult {
    if merchant == required {
        Ok(())
    } else {
        Err(denied("MERCHANT_NOT_ALLOWED", "Merchant is not permitted."))
    }
}
pub fn require_recipient(recipient: &str, required: &str) -> PolicyResult {
    if recipient == required {
        Ok(())
    } else {
        Err(denied(
            "RECIPIENT_NOT_ALLOWED",
            "Recipient is not permitted.",
        ))
    }
}
pub fn is_one_of(value: &str, allowed: &[&str]) -> Result<bool, PolicyError> {
    if allowed.is_empty() || allowed.len() > 32 {
        return Err(denied("INVALID_POLICY", "Use a list of 1 to 32 strings."));
    }
    Ok(allowed.contains(&value))
}
#[cfg(feature = "oracle-ledger")]
pub fn cap_purchase_tiers(
    amount_units: u64,
    token: &str,
    purchase_counts: &Option<Vec<u64>>,
    maximum_units: u64,
    first_count: u64,
    currency: &str,
    decimals: u64,
) -> PolicyResult {
    denomination(token, currency, decimals)?;
    crate::spending::check_tier_values(
        amount_units,
        purchase_counts.as_deref(),
        maximum_units,
        first_count,
    )
    .map_err(|e| denied(&e.code, &e.reason))
}
#[cfg(not(feature = "oracle-ledger"))]
pub fn cap_purchase_tiers(
    _amount_units: u64,
    _token: &str,
    _purchase_counts: &Option<Vec<u64>>,
    _maximum_units: u64,
    _first_count: u64,
    _currency: &str,
    _decimals: u64,
) -> PolicyResult {
    Err(denied(
        "LEDGER_REQUIRED",
        "Purchase tiers require the oracle ledger feature.",
    ))
}

/// Read only the named trusted numeric evidence. An absent value is not an approval.
pub fn preference_evidence(
    ctx: &crate::Context,
    question: &str,
) -> Option<crate::ConfidenceInterval> {
    if ctx.original_intent.trim().is_empty() {
        return None;
    }
    ctx.confidence
        .get(&crate::semantic_evidence_key(question))
        .copied()
}
/// Threshold checks use only numeric evidence. The oracle supplies authenticated owner continuation.
#[cfg(feature = "std")]
pub async fn check_preference(
    evidence: Option<crate::ConfidenceInterval>,
    question: &str,
    deny: impl Into<crate::v1::prelude::Threshold>,
    approve: impl Into<crate::v1::prelude::Threshold>,
) -> PolicyResult {
    let (deny, below) = crate::v1::prelude::threshold(deny.into(), "deny")?;
    let (approve, above) = crate::v1::prelude::threshold(approve.into(), "approve")?;
    let lower = crate::prelude::percent(&below)?;
    let upper = crate::prelude::percent(&above)?;
    if deny && approve && lower >= upper {
        return Err(denied("INVALID_POLICY", "Denial must be below approval."));
    }
    if question.trim().is_empty() || question.len() > 1024 {
        return Err(denied(
            "INVALID_POLICY",
            "Use a preference question of 1–1,024 bytes.",
        ));
    }
    if deny || approve {
        let fit = evidence.ok_or_else(|| {
            denied(
                "SEMANTIC_EVIDENCE_REQUIRED",
                "A preference assessment is required.",
            )
        })?;
        if fit.lower_bps > fit.upper_bps || fit.upper_bps > 10_000 {
            return Err(denied("INVALID_EVIDENCE", "Confidence bounds are invalid."));
        }
        if deny && fit.upper_bps <= lower {
            return crate::prelude::fail("The request does not meet this preference.");
        }
        if approve && fit.lower_bps >= upper {
            return Ok(());
        }
    }
    Err(denied(
        "USER_INPUT_REQUIRED",
        "Use the trusted oracle to obtain an authenticated answer.",
    ))
}

/// Owner-controlled native limit descriptor. Only the native adapter can supply current values.
#[derive(Clone, Copy)]
pub struct OwnerLimit {
    key: &'static str,
    initial_units: u64,
}
/// Declare a native account field and its installation value in the policy constructor.
pub fn owner_limit(key: &'static str, initial_units: u64) -> OwnerLimit {
    OwnerLimit { key, initial_units }
}
/// Read verified current account state. Request JSON and runtime_context are not policy storage.
pub fn stored_limit(ctx: &crate::Context, limit: OwnerLimit) -> Result<u64, PolicyError> {
    if limit.initial_units == 0 || limit.initial_units > 50_000_000 {
        return Err(denied(
            "INVALID_POLICY",
            "The initial native limit is outside its supported range.",
        ));
    }
    let storage = ctx.native_policy_storage.as_ref().ok_or_else(|| {
        denied(
            "NATIVE_STORAGE_REQUIRED",
            "Verified native policy storage is required.",
        )
    })?;
    if storage.daily_limit_units > 50_000_000 || storage.action_limit_units > 50_000_000 {
        return Err(denied(
            "INVALID_NATIVE_STORAGE",
            "Native limit exceeds its supported range.",
        ));
    }
    match limit.key {
        "native_daily_limit" => Ok(storage.daily_limit_units),
        "native_action_limit" => Ok(storage.action_limit_units),
        _ => Err(denied("INVALID_POLICY", "Unknown native storage field.")),
    }
}
