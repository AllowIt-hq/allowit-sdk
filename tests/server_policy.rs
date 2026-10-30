#![cfg(feature = "compiler")]
use allowit_sdk::{compile, compile_stellar_policy, server_policy::*};

const POLICY: &str = r#"use allowit::prelude::*;
async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::require_asset(ctx, "USDC")?;
    Ok(())
}
async fn _server_execute(ctx: &Context) -> PolicyResult {
    let exposure = allowit::context_u64(ctx, "vault_economic_exposure_units")?;
    if exposure > 1_000_000_000 { return allowit::fail("Vault exposure exceeds the owner limit."); }
    Ok(())
}"#;

#[test]
fn same_source_server_hook_is_separate_and_bound_to_full_source() {
    let native = compile(POLICY).unwrap();
    let hook = compile_stellar_server_policy(POLICY).unwrap().unwrap();
    assert_eq!(native.source_hash, hook.owner_source_hash);
    assert_eq!(
        hook.policy.execution_requirements.context_u64_keys,
        ["vault_economic_exposure_units"]
    );
    assert!(native.execution_requirements.context_u64_keys.is_empty());
    compile_stellar_policy(POLICY).unwrap();
    let changed = POLICY.replace("1_000_000_000", "2_000_000_000");
    assert_ne!(native.source_hash, compile(&changed).unwrap().source_hash);
    let onchain = split_stellar_source(POLICY).unwrap();
    assert_eq!(onchain.len(), POLICY.len());
    assert!(!onchain.contains("_server_execute"));
    assert!(onchain.contains("_execute"));
}

#[test]
fn server_hook_cannot_disappear_or_hide_invalid_code() {
    for invalid in [
        POLICY.replace("async fn _server_execute", "pub async fn _server_execute"),
        POLICY.replace("async fn _server_execute", "fn _server_execute"),
        POLICY.replace(
            "let exposure =",
            "let exposure = unknown::oops(); let extra =",
        ),
        format!("{POLICY}\nasync fn _server_execute(ctx: &Context) -> PolicyResult {{ Ok(()) }}"),
    ] {
        assert!(compile(&invalid).is_err());
        assert!(compile_stellar_server_policy(&invalid).is_err());
    }
}

#[test]
fn absence_preserves_existing_source_and_ir() {
    let source = "async fn _execute(ctx: &Context) -> PolicyResult { Ok(()) }";
    assert!(compile_stellar_server_policy(source).unwrap().is_none());
    assert_eq!(split_stellar_source(source).unwrap(), source);
}

#[test]
fn explicit_economic_guard_evaluates_as_a_valid_server_artifact() {
    let hook = compile_stellar_server_policy(POLICY).unwrap().unwrap();
    let mut context = allowit_sdk::Context {
        allocation_units: u64::MAX,
        amount_units: 1,
        token: "XLM".into(),
        network: "stellar:testnet".into(),
        action: "deposit".into(),
        runtime_context: serde_json::json!({"vault_economic_exposure_units": 1_000_000_000u64}),
        ..Default::default()
    };
    let decision = evaluate_stellar_server_policy(&hook, &context);
    assert_eq!(decision.outcome, "pass", "{decision:?}");
    context.runtime_context["vault_economic_exposure_units"] = serde_json::json!(1_000_000_001u64);
    assert_eq!(
        evaluate_stellar_server_policy(&hook, &context).outcome,
        "fail"
    );
    context.runtime_context = serde_json::json!({});
    assert_eq!(
        evaluate_stellar_server_policy(&hook, &context).outcome,
        "fail"
    );
}
