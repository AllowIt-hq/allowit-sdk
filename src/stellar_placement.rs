//! Deterministic authoring-time placement for eligible owner-controlled rules.
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
