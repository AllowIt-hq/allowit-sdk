//! Standard native preflight contains only the explicit initial Grant-rule prefix.
//! Complete authored policy code also runs offchain with authenticated context.
use crate::{CompileError, PolicyKey, PolicyScalar};
use std::collections::BTreeMap;
pub const STELLAR_ARGUMENT_SCHEMA:&[u8]=b"allowit.stellar.sep41_transfer.v1:ScValMap(amount:i128,from:Address,to:Address,token:Address)";
pub const STELLAR_SOURCE_PROFILE: &str = "allowit-stellar-std-prefix-v1";
#[derive(Clone, Debug)]
pub struct StellarPolicyContext {
    pub asset: String,
    pub recipient: String,
    pub token: String,
    pub decimals: u32,
    pub amount: i128,
    pub spent_daily: i128,
    pub spent_total: i128,
    pub calls: u64,
    pub now: u64,
    pub spent_day: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionPlacement {
    pub function: String,
    pub location: String,
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StellarSourceProfile {
    pub profile: String,
    pub version: u32,
    pub source_hash: [u8; 32],
    pub public_entrypoint: String,
    pub private_entrypoint: String,
    pub argument_schema_digest: [u8; 32],
    pub admitted_operations: Vec<String>,
    pub permitted_effects: Vec<String>,
    pub native_prefix: Vec<String>,
    pub offchain_required: bool,
    pub functions: Vec<FunctionPlacement>,
}
#[derive(Clone, Debug)]
pub struct StellarPolicy {
    pub source_profile: StellarSourceProfile,
    pub source_hash: [u8; 32],
    pub argument_schema_hash: [u8; 32],
    pub rust_source: String,
    pub property_keys: Vec<PolicyKey>,
    pub grant_keys: Vec<PolicyKey>,
    pub engine_checks: Vec<String>,
    pub explicit_engine_checks: Vec<String>,
    pub placement_manifest: crate::PlacementManifest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StellarEvaluation {
    Approved,
    Denied,
    RequiresNativeEngine,
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}
pub fn compile_stellar_policy(source: &str) -> Result<StellarPolicy, CompileError> {
    compile_stellar_policy_with_options(source, &crate::StellarCompileOptions::default())
}
pub fn compile_stellar_policy_with_options(
    source: &str,
    options: &crate::StellarCompileOptions,
) -> Result<StellarPolicy, CompileError> {
    if options.default_placement == crate::RulePlacement::Shared
        || options.expected_transactions.is_some()
        || options.cost_context.is_some()
        || !options.cost_estimates.is_empty()
    {
        return Err(CompileError::new(
            "UNSUPPORTED_RULE_PLACEMENT",
            "Standard preflight placement follows the explicit initial Grant-rule prefix.",
        ));
    }
    let compiled = crate::compile(source)?;
    let prefix = compiled.native_prefix.clone();
    let source_hash = digest(source.as_bytes());
    let argument_schema_hash = digest(STELLAR_ARGUMENT_SCHEMA);
    let checks = prefix
        .iter()
        .map(|rule| crate::CheckPlacement {
            rule_id: rule.clone(),
            authority: crate::RuleAuthority::OwnerPure,
            requested: crate::RulePlacement::Auto,
            selected: crate::RulePlacement::Local,
            reason: "Explicit consecutive initial call using its same-named Grant reference".into(),
        })
        .collect();
    let placement_manifest = crate::PlacementManifest::new(options, checks);
    let mut prefix_calls = prefix.iter();
    let mut next = prefix_calls.next();
    let functions = compiled
        .calls
        .iter()
        .filter(|call| {
            crate::rule_values::RULES.contains(&call.name.rsplit("::").next().unwrap_or(""))
        })
        .map(|call| {
            let name = call.name.rsplit("::").next().unwrap_or("");
            let native = next.is_some_and(|expected| expected == name);
            if native {
                next = prefix_calls.next();
            }
            FunctionPlacement {
                function: format!("allowit::{name}"),
                location: if native {
                    "standard_preflight"
                } else {
                    "offchain"
                }
                .into(),
                start: call.start,
                end: call.end,
            }
        })
        .collect();
    let mut body = String::new();
    for rule in &prefix {
        body += &format!(
            "    let result=allowit_policy_rules::{rule}(ctx.asset.env().clone(),host.request(),EngineState{{grant:grant.clone(),spent:working.clone()}}).map_err(|_|())?;\n    if !result.approval{{return Err(());}}\n    working=result.spent;\n"
        );
    }
    let rust_source = format!(
        "// Generated standard preflight only. Full policy code executes offchain.\n#[allow(unused_imports)]\nuse allowit_wallet_protocol::{{PolicyContext,Grant,Spent,EngineState,PolicyEngine}};\npub const SOURCE_PROFILE:&str={STELLAR_SOURCE_PROFILE:?};\npub const SOURCE_PROFILE_VERSION:u32=1;\npub const ADMITTED_OPERATION:&str=\"stellar::sep41_transfer\";\npub const SOURCE_HASH:[u8;32]={source_hash:?};\npub const ARGUMENT_SCHEMA_HASH:[u8;32]={argument_schema_hash:?};\npub fn execute(ctx:&PolicyContext,grant:&Grant,state:&EngineState,host:&impl PolicyEngine)->Result<Spent,()>{{_execute(ctx,grant,state,host)}}\n#[allow(unused_variables,unused_mut)]\nfn _execute(ctx:&PolicyContext,grant:&Grant,state:&EngineState,host:&impl PolicyEngine)->Result<Spent,()>{{\n    if ctx.amount<=0{{return Err(());}}\n    let mut working=state.spent.clone();\n{body}    Ok(working)\n}}\n"
    );
    Ok(StellarPolicy {
        source_profile: StellarSourceProfile {
            profile: STELLAR_SOURCE_PROFILE.into(),
            version: 1,
            source_hash,
            public_entrypoint: "execute".into(),
            private_entrypoint: "_execute".into(),
            argument_schema_digest: argument_schema_hash,
            admitted_operations: vec!["stellar::sep41_transfer".into()],
            permitted_effects: vec![
                "bound_asset_debit".into(),
                "recipient_credit".into(),
                "policy_spent_output".into(),
            ],
            native_prefix: prefix,
            offchain_required: compiled.offchain_required,
            functions,
        },
        source_hash,
        argument_schema_hash,
        rust_source,
        property_keys: vec![],
        grant_keys: vec![],
        engine_checks: vec![],
        explicit_engine_checks: vec![],
        placement_manifest,
    })
}
pub fn compile_stellar_deposit_policy(source: &str) -> Result<StellarPolicy, CompileError> {
    let mut policy = compile_stellar_policy(source)?;
    let hash = digest(crate::stellar_vault::DEPOSIT_ARGUMENT_SCHEMA);
    policy.argument_schema_hash = hash;
    policy.source_profile.argument_schema_digest = hash;
    policy.source_profile.admitted_operations =
        vec![crate::stellar_vault::DEPOSIT_OPERATION.into()];
    policy.source_profile.permitted_effects = vec![
        "bound_asset_debit".into(),
        "vault_share_credit".into(),
        "policy_spent_output".into(),
    ];
    policy.rust_source = policy
        .rust_source
        .replace(
            "pub const ADMITTED_OPERATION:&str=\"stellar::sep41_transfer\"",
            "pub const ADMITTED_OPERATION:&str=\"stellar::sep56_deposit\"",
        )
        .replace(
            &format!(
                "pub const ARGUMENT_SCHEMA_HASH:[u8;32]={:?};",
                digest(STELLAR_ARGUMENT_SCHEMA)
            ),
            &format!("pub const ARGUMENT_SCHEMA_HASH:[u8;32]={hash:?};"),
        );
    Ok(policy)
}
impl StellarPolicy {
    pub(crate) fn common_rules(&self) -> Option<Vec<String>> {
        Some(self.source_profile.native_prefix.clone())
    }
    pub fn evaluate_classic(&self, c: &StellarPolicyContext) -> bool {
        c.amount > 0
            && self.source_profile.native_prefix.is_empty()
            && !self.source_profile.offchain_required
    }
    pub fn evaluate(&self, c: &StellarPolicyContext) -> bool {
        self.evaluate_classic(c)
    }
    pub fn evaluate_operation(&self, c: &StellarPolicyContext, operation: &str) -> bool {
        self.source_profile
            .admitted_operations
            .iter()
            .any(|op| op == operation)
            && self.evaluate_classic(c)
    }
    pub fn requires_native_preflight(&self) -> bool {
        !self.source_profile.native_prefix.is_empty()
    }
    pub fn uses_named_state(&self) -> bool {
        false
    }
    pub fn evaluation_with_state(
        &self,
        c: &StellarPolicyContext,
        _properties: &mut BTreeMap<String, PolicyScalar>,
        _grant: &BTreeMap<String, PolicyScalar>,
    ) -> StellarEvaluation {
        if self.requires_native_preflight() || self.source_profile.offchain_required {
            StellarEvaluation::RequiresNativeEngine
        } else if self.evaluate(c) {
            StellarEvaluation::Approved
        } else {
            StellarEvaluation::Denied
        }
    }
    pub fn evaluate_with_state(
        &self,
        c: &StellarPolicyContext,
        properties: &mut BTreeMap<String, PolicyScalar>,
        grant: &BTreeMap<String, PolicyScalar>,
    ) -> bool {
        self.evaluation_with_state(c, properties, grant) == StellarEvaluation::Approved
    }
}
