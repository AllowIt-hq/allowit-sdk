//! Wallet implementation recommendations from the complete admitted Stellar policy.
use crate::{CompileError, StellarPolicy, compile_stellar_policy};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEngine {
    SimplePolicyEngine,
    CommonPolicyEngine,
    CustomPolicy,
}

impl PolicyEngine {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::SimplePolicyEngine => "simple_policy_engine",
            Self::CommonPolicyEngine => "common_policy_engine",
            Self::CustomPolicy => "custom_policy",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StellarRail {
    Classic,
    PolicyWallet,
}

/// A compatible implementation, not permission to replace owner-required enforcement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EngineRecommendation {
    pub engine: PolicyEngine,
    pub rail: StellarRail,
    pub reason: String,
    /// True only when the selected wallet enforces the entire authored policy onchain.
    pub onchain_policy: bool,
    /// Exact ordered Common selectors. Other implementations use no selector list.
    pub rules: Vec<String>,
}

/// Recommend compatible implementations without reducing policy meaning or changing its source.
/// Classic accounts execute no custom Soroban code. Their policy enforcement belongs to the signing service.
pub fn recommend_stellar_engines(source: &str) -> Result<Vec<EngineRecommendation>, CompileError> {
    let policy = compile_stellar_policy(source)?;
    Ok(policy.engine_recommendations())
}

/// Select an explicitly requested compatible engine. No failed lowering triggers a weaker fallback.
pub fn select_stellar_engine(
    source: &str,
    rail: StellarRail,
    engine: PolicyEngine,
    require_onchain: bool,
) -> Result<EngineRecommendation, CompileError> {
    recommend_stellar_engines(source)?.into_iter().find(|choice| choice.engine==engine && choice.rail==rail && (!require_onchain || choice.onchain_policy)).ok_or_else(|| CompileError::new("INCOMPATIBLE_POLICY_ENGINE", "The selected rail and engine cannot provide the required complete policy enforcement."))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StellarWalletCompilation {
    pub source_hash: [u8; 32],
    pub selection: EngineRecommendation,
    /// Only CustomPolicy needs source-specific Wasm. Fixed engines use their released artifact and owner configuration.
    pub generated_source: Option<String>,
}

pub fn compile_stellar_wallet(
    source: &str,
    network: &str,
    rail: StellarRail,
    engine: PolicyEngine,
    require_onchain: bool,
) -> Result<StellarWalletCompilation, CompileError> {
    if !matches!(network, "stellar:testnet" | "stellar:mainnet") {
        return Err(CompileError::new(
            "INCOMPATIBLE_POLICY_ENGINE",
            "Policy-wallet implementations require a supported Stellar network.",
        ));
    }
    let policy = compile_stellar_policy(source)?;
    let selection=policy.engine_recommendations().into_iter().find(|choice| choice.engine==engine && choice.rail==rail && (!require_onchain || choice.onchain_policy)).ok_or_else(|| CompileError::new("INCOMPATIBLE_POLICY_ENGINE", "The selected rail and engine cannot provide the required complete policy enforcement."))?;
    Ok(StellarWalletCompilation {
        source_hash: policy.source_hash,
        selection,
        generated_source: (engine == PolicyEngine::CustomPolicy).then_some(policy.rust_source),
    })
}

impl StellarPolicy {
    pub fn engine_recommendations(&self) -> Vec<EngineRecommendation> {
        let mut choices = Vec::new();
        if let Some(rules) = self.common_rules() {
            choices.push(EngineRecommendation {
                engine: if rules.is_empty() {
                    PolicyEngine::SimplePolicyEngine
                } else {
                    PolicyEngine::CommonPolicyEngine
                },
                rail: StellarRail::PolicyWallet,
                reason: if rules.is_empty() {
                    "The standard wallet enforces custody and authorization. The full policy runs offchain."
                } else {
                    "The standard wallet enforces the explicit initial Grant-rule prefix. Remaining policy code runs offchain."
                }
                .into(),
                onchain_policy: !self.source_profile.offchain_required,
                rules,
            });
        }
        if choices
            .first()
            .is_some_and(|choice| choice.engine == PolicyEngine::SimplePolicyEngine)
        {
            choices.push(EngineRecommendation {
                engine: PolicyEngine::CommonPolicyEngine,
                rail: StellarRail::PolicyWallet,
                reason: "The pass-through policy is compatible with an empty Common rule list."
                    .into(),
                onchain_policy: !self.source_profile.offchain_required,
                rules: Vec::new(),
            });
        }
        choices.push(EngineRecommendation {
            engine: PolicyEngine::CustomPolicy,
            rail: StellarRail::PolicyWallet,
            reason:
                "Compile only the declared standard prefix into owner-controlled code. Remaining policy code runs offchain."
                    .into(),
            onchain_policy: !self.source_profile.offchain_required,
            rules: Vec::new(),
        });
        if self.source_profile.native_prefix.is_empty() {
            for rail in [StellarRail::Classic] {
                if choices
                    .iter()
                    .any(|c| c.engine == PolicyEngine::SimplePolicyEngine && c.rail == rail)
                {
                    continue;
                }
                choices.push(EngineRecommendation {
                    engine: PolicyEngine::SimplePolicyEngine,
                    rail,
                    reason: if rail == StellarRail::Classic { "Agent and service signatures enforce the policy offchain. Classic reserves apply; no Soroban custody rent is required." } else { "The signing service enforces business rules; the wallet enforces both signatures and custody invariants." }.into(),
                    onchain_policy: false,
                    rules: Vec::new(),
                });
            }
        }
        choices
    }
}
