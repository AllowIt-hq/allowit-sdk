//! Deterministic authoring-time placement for eligible owner-controlled rules.
use crate::CompileError;
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
    if authority == RuleAuthority::System
        || requested == RulePlacement::Shared
        || options.default_placement == RulePlacement::Shared
        || options.expected_transactions.is_some()
        || options.cost_context.is_some()
        || !options.cost_estimates.is_empty()
    {
        return Err(invalid(
            "Hybrid and cost-based rule placement is deferred. Use a fixed policy engine or local CustomPolicy mixins.",
        ));
    }
    Ok(CheckPlacement {
        rule_id: rule_id.into(),
        authority,
        requested,
        selected: RulePlacement::Local,
        reason: "Owner-controlled policy implementation uses local rule mixins".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_local_rules_are_available() {
        assert_eq!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Auto,
                &StellarCompileOptions::default()
            )
            .unwrap()
            .selected,
            RulePlacement::Local
        );
        assert!(
            plan(
                "daily_cap",
                RuleAuthority::OwnerPure,
                RulePlacement::Shared,
                &StellarCompileOptions::default()
            )
            .is_err()
        );
        assert!(
            plan(
                "daily_cap",
                RuleAuthority::System,
                RulePlacement::Local,
                &StellarCompileOptions::default()
            )
            .is_err()
        );
    }
}
