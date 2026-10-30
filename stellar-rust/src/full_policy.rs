//! Complete source evaluation is an explicit service capability, separate from Std preflight.
use crate::*;
use allowit_policy_sdk::{Context, PolicyScalar};

/// Project only canonical owner rule inputs. Native values retain their exact identities and units.
pub fn policy_grant_values(
    snapshot: &PolicyStateSnapshot,
) -> Result<std::collections::BTreeMap<String, PolicyScalar>> {
    let mut values = std::collections::BTreeMap::new();
    for rule in allowit_policy_sdk::rule_values::RULES {
        let Some(value) = snapshot.grant.0.get(*rule) else {
            continue;
        };
        let value = match value {
            ScVal::Void => PolicyScalar::Null,
            ScVal::U64(n) => PolicyScalar::U64(*n),
            ScVal::I128(parts) => PolicyScalar::U64(
                u64::try_from((i128::from(parts.hi) << 64) | i128::from(parts.lo))
                    .map_err(|_| "Owner rule limit exceeds supported offchain units")?,
            ),
            ScVal::Vec(Some(entries)) if *rule == "tokens" || *rule == "recipients" => {
                PolicyScalar::Strings(entries.iter().map(rpc::addr).collect::<Result<Vec<_>>>()?)
            }
            _ => PolicyScalar::Unsupported,
        };
        values.insert((*rule).into(), value);
    }
    Ok(values)
}
/// The host supplies authoritative actual Spent; the SDK binds its operation facts and Grant to this packet.
/// A failed evaluation restores every working property. Success returns only a proposed working map.
pub fn evaluate_policy_context(p: &PreparedOperation, context: &Context) -> Result<()> {
    let c = &p.config;
    let operation = operation_from_request(c, &p.request)?;
    let movement = operation.movement();
    let amount = u64::try_from(movement.amount)
        .map_err(|_| "Native operation amount exceeds supported offchain units")?;
    let facts = context
        .rule_facts
        .as_ref()
        .ok_or("Authenticated policy rule facts are required")?;
    if context.network != c.network
        || context.token != c.asset_symbol
        || context.action != operation.name()
        || context.amount_units != amount
        || context.recipient != movement.recipient
        || context.now != p.created_at
        || facts.now != p.created_at
        || facts.amount_units != amount
        || facts.asset_id != c.asset
        || facts.recipient != movement.recipient
        || facts.operation_id != hex(&p.request.operation.nonce)
    {
        return Err("Full policy context differs from the exact native request".into());
    }
    let state = context
        .policy_state
        .as_ref()
        .ok_or("Authoritative policy accounting is required")?;
    if c.route == WalletRoute::Native {
        if facts.grant_available || !state.grant.is_empty() {
            return Err("Native custody has no owner Grant map".into());
        }
    } else if c.route == WalletRoute::PolicyWallet {
        let snapshot = p
            .state
            .policy_state
            .as_ref()
            .ok_or("Fresh native owner Grant is required")?;
        if !facts.grant_available || state.grant != policy_grant_values(snapshot)? {
            return Err("Policy Grant differs from fresh native owner values".into());
        }
    } else {
        return Err("Historical wallets cannot execute fresh policy code".into());
    }
    if let Some(actual) = &facts.expense {
        let bound = p
            .request
            .expense
            .as_ref()
            .ok_or("Policy expense facts have no signed native ceiling")?;
        if i128::from(actual.spent_units) > bound.spent_units
            || i128::from(actual.gas_units) > bound.gas_units
        {
            return Err("Policy expenses exceed signed native ceilings".into());
        }
    }
    let policy = allowit_policy_sdk::compile(&c.policy_source).map_err(err)?;
    if policy.source_hash != hex(&c.source_hash) {
        return Err("Owner-approved full source identity differs".into());
    }
    let decision = allowit_policy_sdk::evaluate_with_policy_rules(&policy, context);
    if decision.outcome != "pass" {
        return Err(format!("{}: {}", decision.code, decision.reason));
    }
    Ok(())
}
/// Service signing requires complete source evaluation; the resulting signature binds exact source and effects.
pub fn sign_with_policy_context(
    p: &PreparedOperation,
    seed: &[u8; 32],
    now: u64,
    context: &Context,
) -> Result<DetachedSignature> {
    validate_prepared(p, now)?;
    current_authorization_bounds(p)?;
    evaluate_policy_context(p, context)?;
    if p.config.route == WalletRoute::PolicyWallet {
        return sign(p, seed, now);
    }
    sign_unchecked_role(p, seed)
}
