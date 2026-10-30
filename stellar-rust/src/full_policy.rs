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
            ScVal::I128(parts) => {
                u64::try_from((i128::from(parts.hi) << 64) | i128::from(parts.lo))
                    .map(PolicyScalar::U64)
                    .unwrap_or(PolicyScalar::Unsupported)
            }
            ScVal::Vec(Some(entries)) if *rule == "tokens" || *rule == "recipients" => entries
                .iter()
                .map(rpc::addr)
                .collect::<Result<Vec<_>>>()
                .map(PolicyScalar::Strings)
                .unwrap_or(PolicyScalar::Unsupported),
            _ => PolicyScalar::Unsupported,
        };
        values.insert((*rule).into(), value);
    }
    Ok(values)
}
/// The host supplies authoritative actual Spent; the SDK binds its operation facts and Grant to this packet.
/// Validation evaluates an independent context copy. It never commits caller properties.
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
    let validation_context = context.clone();
    let decision = allowit_policy_sdk::evaluate_with_policy_rules(&policy, &validation_context);
    if decision.outcome != "pass" {
        return Err(format!("{}: {}", decision.code, decision.reason));
    }
    if let Some(deadline) = *validation_context.policy_deadline.borrow() {
        // Stellar Core maxTime is inclusive; the native policy rule deadline is exclusive.
        let maximum = deadline
            .checked_sub(1)
            .ok_or("Policy deadline cannot admit a native transaction")?;
        if p.expires_at > maximum {
            return Err("POLICY_EXPIRY_BOUNDS_REQUIRED: Bound the final operation to its reached exclusive policy deadline".into());
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use allowit_policy_sdk::{PolicyRuleFacts, TrustedPolicyState};
    #[test]
    fn malformed_unread_grants_remain_values_until_a_rule_executes() {
        let snapshot = PolicyStateSnapshot {
            grant: NativeValues(std::collections::BTreeMap::from([
                (
                    "spent_cap".into(),
                    ScVal::I128(Int128Parts {
                        hi: -1,
                        lo: u64::MAX,
                    }),
                ),
                ("expiry".into(), ScVal::I128(Int128Parts { hi: 1, lo: 0 })),
                (
                    "tokens".into(),
                    ScVal::Vec(Some(vec![ScVal::U64(7)].try_into().unwrap())),
                ),
            ])),
            spent: NativeValues::default(),
        };
        let grant = policy_grant_values(&snapshot).unwrap();
        assert!(
            grant
                .values()
                .all(|value| value == &PolicyScalar::Unsupported)
        );
        let mut ctx = Context::default();
        ctx.token = "XLM".into();
        ctx.action = "stellar::sep41_transfer".into();
        ctx.policy_state = Some(TrustedPolicyState {
            grant,
            properties: Default::default(),
        });
        ctx.rule_facts = Some(PolicyRuleFacts {
            operation_id: "op".into(),
            grant_available: true,
            amount_units: 1,
            asset_id: "SAC".into(),
            recipient: "RECIPIENT".into(),
            now: 10,
            expense: None,
        });
        for body in [
            "Ok(())",
            "if false{allowit::spent_cap(grant::spent_cap)?;allowit::tokens(grant::tokens)?;}Ok(())",
        ] {
            let source = format!(
                "use allowit::prelude::*;async fn _execute(ctx:&Context)->PolicyResult{{{body}}}"
            );
            assert_eq!(
                allowit_policy_sdk::evaluate_with_policy_rules(
                    &allowit_policy_sdk::compile(&source).unwrap(),
                    &ctx
                )
                .outcome,
                "pass"
            );
        }
        for rule in ["spent_cap", "expiry", "tokens"] {
            let source = format!(
                "use allowit::prelude::*;async fn _execute(ctx:&Context)->PolicyResult{{allowit::{rule}(grant::{rule})?;Ok(())}}"
            );
            assert_eq!(
                allowit_policy_sdk::evaluate_with_policy_rules(
                    &allowit_policy_sdk::compile(&source).unwrap(),
                    &ctx
                )
                .code,
                "INVALID_GRANT"
            );
        }
    }
}
