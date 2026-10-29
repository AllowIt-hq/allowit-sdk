//! Deterministic authoring-time placement for eligible owner-controlled rules.
use crate::{CompileError, REGISTRY_VERSION, STELLAR_SOURCE_PROFILE};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulePlacement {
    Auto,
    Local,
    Shared,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleAuthority {
    OwnerPure,
    System,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostContext {
    /// Target network used by both local and shared cost observations.
    pub network: String,
    /// One normalized comparable unit for deployment and execution costs.
    pub cost_unit: String,
    /// Exact canonical rule-body identity measured by the estimate provider.
    pub rules_digest: [u8; 32],
    /// Source profile for which these observations are compatible.
    pub source_profile: String,
    /// Source registry used by the measured artifacts.
    pub registry_version: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleCostEstimate {
    /// Registered rule identifier. It never supplies a runtime dispatch target.
    pub rule_id: String,
    /// Complete measurement context; must equal the selected authoring context.
    pub context: CostContext,
    /// Bundled deployment cost, in context.cost_unit.
    pub local_deployment_cost: u64,
    /// Shared-placement deployment cost, in the same unit.
    pub shared_deployment_cost: u64,
    /// Expected bundled invocation cost, in the same unit.
    pub local_per_call_cost: u64,
    /// Expected shared invocation cost, in the same unit.
    pub shared_per_call_cost: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StellarCompileOptions {
    /// Strategy for generic owner-rule calls. Explicit engine/self requests take precedence.
    pub default_placement: RulePlacement,
    /// Expected rule execution volume. None means unknown, not zero.
    pub expected_transactions: Option<u64>,
    /// Target measurement context selected by the author.
    pub cost_context: Option<CostContext>,
    /// Optional compatible supplied estimates; the compiler does not invent prices.
    pub cost_estimates: Vec<RuleCostEstimate>,
}
impl Default for StellarCompileOptions {
    fn default() -> Self {
        Self {
            default_placement: RulePlacement::Auto,
            expected_transactions: None,
            cost_context: None,
            cost_estimates: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckPlacement {
    /// Registered rule identifier. It never supplies a runtime dispatch target.
    pub rule_id: String,
    /// Registry-owned authority classification. Estimates cannot change it.
    pub authority: RuleAuthority,
    /// Placement requested by the source spelling.
    pub requested: RulePlacement,
    /// Fixed authoring result: Local or Shared.
    pub selected: RulePlacement,
    /// Concrete selection reason, including any missing or incompatible estimates.
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementManifest {
    /// Exact authoring options used by this artifact.
    pub options: StellarCompileOptions,
    /// Source-ordered planned checks, including all branches.
    pub checks: Vec<CheckPlacement>,
    /// Conservative policy-wide trust flag. A shared adopted result can replace the full spent map.
    pub possible_shared_spent_writer: bool,
}
impl PlacementManifest {
    pub(crate) fn new(options: &StellarCompileOptions, checks: Vec<CheckPlacement>) -> Self {
        let possible_shared_spent_writer = checks
            .iter()
            .any(|c| c.selected == RulePlacement::Shared && c.rule_id != "amount_at_most");
        Self {
            options: options.clone(),
            checks,
            possible_shared_spent_writer,
        }
    }
}
/// Closed native-rule eligibility registry. New rules require an explicit authority assessment.
pub(crate) fn authority(rule_id: &str) -> Option<RuleAuthority> {
    [
        "daily_cap",
        "transaction_cap",
        "total_cap",
        "call_cap",
        "recipient",
        "expiry",
        "token",
        "cap_per_transaction",
        "cap_per_day",
        "amount_at_most",
        "recipient_is",
        "asset_label",
        "actions",
    ]
    .contains(&rule_id)
    .then_some(RuleAuthority::OwnerPure)
}
fn invalid(message: &str) -> CompileError {
    CompileError::new("INVALID_RULE_PLACEMENT", message)
}
pub(crate) fn plan(
    rule_id: &str,
    authority: RuleAuthority,
    requested: RulePlacement,
    options: &StellarCompileOptions,
) -> Result<CheckPlacement, CompileError> {
    let (selected, reason) = match requested {
        RulePlacement::Shared => (RulePlacement::Shared, "Explicit shared placement"),
        RulePlacement::Local if authority == RuleAuthority::System => {
            return Err(invalid(
                "System authority checks cannot be bundled locally.",
            ));
        }
        RulePlacement::Local => (
            RulePlacement::Local,
            "Explicit eligible owner-rule placement",
        ),
        RulePlacement::Auto if authority == RuleAuthority::System => (
            RulePlacement::Shared,
            "System authority requires shared placement",
        ),
        RulePlacement::Auto => {
            let requested = options.default_placement;
            if requested != RulePlacement::Auto {
                return plan(rule_id, authority, requested, options).map(|mut p| {
                    p.requested = RulePlacement::Auto;
                    p.reason = "Authoring option selected the eligible owner-rule placement".into();
                    p
                });
            }
            let fallback = (
                RulePlacement::Local,
                "Missing compatible cost estimates; owner authority default",
            );
            match (options.expected_transactions, options.cost_context.as_ref()) {
                (Some(volume), Some(context)) => {
                    let estimates = options
                        .cost_estimates
                        .iter()
                        .filter(|e| e.rule_id == rule_id)
                        .collect::<Vec<_>>();
                    if estimates.len() > 1 {
                        return Err(invalid("A rule must have at most one cost estimate."));
                    }
                    if let Some(estimate) = estimates.first() {
                        if estimate.context != *context
                            || context.network.is_empty()
                            || context.cost_unit.is_empty()
                            || context.rules_digest == [0; 32]
                            || context.source_profile != STELLAR_SOURCE_PROFILE
                            || context.registry_version != REGISTRY_VERSION
                        {
                            (
                                RulePlacement::Local,
                                "Incompatible cost context; owner authority default",
                            )
                        } else {
                            let local = volume
                                .checked_mul(estimate.local_per_call_cost)
                                .and_then(|v| v.checked_add(estimate.local_deployment_cost))
                                .ok_or_else(|| invalid("Projected local cost exceeds u64."))?;
                            let shared = volume
                                .checked_mul(estimate.shared_per_call_cost)
                                .and_then(|v| v.checked_add(estimate.shared_deployment_cost))
                                .ok_or_else(|| invalid("Projected shared cost exceeds u64."))?;
                            if local < shared {
                                (RulePlacement::Local, "Lower supplied projected cost")
                            } else if shared < local {
                                (RulePlacement::Shared, "Lower supplied projected cost")
                            } else {
                                (
                                    RulePlacement::Local,
                                    "Supplied projected costs tie; owner authority default",
                                )
                            }
                        }
                    } else {
                        fallback
                    }
                }
                _ => fallback,
            }
        }
    };
    Ok(CheckPlacement {
        rule_id: rule_id.into(),
        authority,
        requested,
        selected,
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> CostContext {
        CostContext {
            network: "stellar:testnet".into(),
            cost_unit: "synthetic_comparable_units".into(),
            rules_digest: [5; 32],
            source_profile: STELLAR_SOURCE_PROFILE.into(),
            registry_version: REGISTRY_VERSION.into(),
        }
    }
    fn options(volume: Option<u64>) -> StellarCompileOptions {
        let context = context();
        StellarCompileOptions {
            default_placement: RulePlacement::Auto,
            expected_transactions: volume,
            cost_context: Some(context.clone()),
            cost_estimates: vec![RuleCostEstimate {
                rule_id: "daily_cap".into(),
                context,
                local_deployment_cost: 100,
                shared_deployment_cost: 0,
                local_per_call_cost: 1,
                shared_per_call_cost: 10,
            }],
        }
    }
    #[test]
    fn auto_uses_only_compatible_supplied_comparable_costs() {
        assert_eq!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Auto,
                &options(Some(1))
            )
            .unwrap()
            .selected,
            RulePlacement::Shared
        );
        assert_eq!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Auto,
                &options(Some(100))
            )
            .unwrap()
            .selected,
            RulePlacement::Local
        );
        let missing = plan(
            "daily_cap",
            RuleAuthority::OwnerPure,
            RulePlacement::Auto,
            &options(None),
        )
        .unwrap();
        assert_eq!(missing.selected, RulePlacement::Local);
        assert!(missing.reason.contains("default"));
        let mut incompatible = options(Some(1));
        incompatible.cost_estimates[0].context.cost_unit = "cpu_counts_not_comparable".into();
        let result = plan(
            "daily_cap",
            RuleAuthority::OwnerPure,
            RulePlacement::Auto,
            &incompatible,
        )
        .unwrap();
        assert_eq!(result.selected, RulePlacement::Local);
        assert!(result.reason.contains("Incompatible"));
    }
    #[test]
    fn authority_and_explicit_placement_override_cost() {
        let opts = options(Some(100));
        assert_eq!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Shared,
                &opts
            )
            .unwrap()
            .selected,
            RulePlacement::Shared
        );
        assert_eq!(
            plan("system", RuleAuthority::System, RulePlacement::Auto, &opts)
                .unwrap()
                .selected,
            RulePlacement::Shared
        );
        assert!(plan("system", RuleAuthority::System, RulePlacement::Local, &opts).is_err());
    }
    #[test]
    fn malformed_projection_and_ambiguous_estimates_reject() {
        let mut opts = options(Some(u64::MAX));
        assert!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Auto,
                &opts
            )
            .is_err()
        );
        opts.expected_transactions = Some(2);
        opts.cost_estimates.push(opts.cost_estimates[0].clone());
        assert!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Auto,
                &opts
            )
            .is_err()
        );
    }
}
