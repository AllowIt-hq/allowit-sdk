use allowit_sdk::{
    Context, ExecutionFeature, Profile, ProviderCallInput, compile, evaluate, process_value,
};
use serde_json::json;

const CALL: &str = "paysh::call(\"air-quality\", \"request\", 1000, 5000000, 100000)";
fn source(body: &str) -> String {
    format!(
        "use allowit::prelude::*; pub async fn execute(ctx: &Context) -> PolicyResult {{ set_cap(ctx, \"100\", \"USDC\")?; {body} }}"
    )
}
fn context() -> Context {
    let mut ctx: Context = serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    ctx.provider_call_input = Some(ProviderCallInput {
        service_id: "air-quality".into(),
        input_key: "request".into(),
        request_digest: "a".repeat(64),
    });
    ctx
}
#[test]
fn qualified_operation_retains_typed_args_requirements_and_executed_effect() {
    let policy = compile(&source(&format!(
        "let admitted = {CALL}; if !admitted {{ return fail(\"Unavailable\"); }} Ok(())"
    )))
    .unwrap();
    assert!(policy.calls.iter().any(|call| call.name == "paysh::call"));
    for feature in [
        ExecutionFeature::ProviderCall,
        ExecutionFeature::PaidHttpCall,
        ExecutionFeature::NativeSettlement,
    ] {
        assert!(policy.execution_requirements.features.contains(&feature));
    }
    let decision = evaluate(&policy, Profile::Oracle, &context());
    assert_eq!(decision.outcome, "pass");
    let effect = &decision.system_operations[0];
    assert_eq!(effect.operation, "paysh::call");
    assert_eq!(effect.request_digest, "a".repeat(64));
    assert_eq!(
        (
            effect.max_payment_units,
            effect.max_swap_lamports,
            effect.max_service_fee_lamports
        ),
        (1000, 5000000, 100000)
    );
}
#[test]
fn unregistered_aliases_wrong_types_and_multiple_calls_fail_compilation() {
    for call in [
        "call(\"air-quality\", \"request\", 1000, 5000000, 100000)",
        "allowit::call(\"air-quality\", \"request\", 1000, 5000000, 100000)",
        "paysh::pay(\"air-quality\", \"request\", 1000, 5000000, 100000)",
        "paysh::swap(\"air-quality\", \"request\", 1000, 5000000, 100000)",
        "paysh::call(\"air-quality\", \"request\", \"1000\", 5000000, 100000)",
    ] {
        assert!(
            compile(&source(&format!("{call}; Ok(())"))).is_err(),
            "{call}"
        );
    }
    assert!(
        compile(&source(&format!(
            "if true {{ let admitted = {CALL}; }} else {{ let admitted = {CALL}; }} Ok(())"
        )))
        .is_err()
    );
}
#[test]
fn missing_forged_or_mismatched_host_binding_never_exposes_effects() {
    let policy = compile(&source(&format!("let admitted = {CALL}; Ok(())"))).unwrap();
    for field in ["missing", "service", "input", "digest"] {
        let mut ctx = context();
        match field {
            "missing" => ctx.provider_call_input = None,
            "service" => ctx.provider_call_input.as_mut().unwrap().service_id = "other".into(),
            "input" => ctx.provider_call_input.as_mut().unwrap().input_key = "other".into(),
            _ => ctx.provider_call_input.as_mut().unwrap().request_digest = "invalid".into(),
        }
        let result = evaluate(&policy, Profile::Oracle, &ctx);
        assert_eq!(result.outcome, "fail");
        assert!(result.system_operations.is_empty());
    }
    let result = process_value(
        json!({"operation":"evaluate","profile":"oracle","source":source(&format!("let admitted = {CALL}; Ok(())")),"context":context()}),
    );
    assert_eq!(result["error"]["code"], "INVALID_CONTEXT");
}
#[test]
fn later_refusal_pause_caps_or_untaken_branch_discard_provider_effects() {
    for body in [
        format!("let admitted = {CALL}; fail(\"Denied\")"),
        format!("let admitted = {CALL}; require_user_input(ctx, \"Approve\").await?; Ok(())"),
        format!("let admitted = {CALL}; cap_per_transaction(ctx, \"0.000001\", \"USDC\")?; Ok(())"),
    ] {
        let result = evaluate(
            &compile(&source(&body)).unwrap(),
            Profile::Oracle,
            &context(),
        );
        assert_ne!(result.outcome, "pass");
        assert!(result.system_operations.is_empty());
    }
    let skipped = evaluate(
        &compile(&source(&format!(
            "if false {{ let admitted = {CALL}; }} Ok(())"
        )))
        .unwrap(),
        Profile::Oracle,
        &context(),
    );
    assert_eq!(skipped.outcome, "pass");
    assert!(skipped.system_operations.is_empty());
    let mut capped = context();
    capped.spent_units = 100_000_000;
    let result = evaluate(
        &compile(&source(&format!("let admitted = {CALL}; Ok(())"))).unwrap(),
        Profile::Oracle,
        &capped,
    );
    assert_eq!(result.outcome, "fail");
    assert!(result.system_operations.is_empty());
}
#[test]
fn every_contract_profile_rejects_provider_effects_including_untaken_branches() {
    for body in [
        format!("let admitted = {CALL}; Ok(())"),
        format!("if false {{ let admitted = {CALL}; }} Ok(())"),
    ] {
        let result = evaluate(
            &compile(&source(&body)).unwrap(),
            Profile::Contract,
            &context(),
        );
        assert_eq!(result.code, "PROVIDER_PROFILE_UNSUPPORTED");
        assert!(result.system_operations.is_empty());
    }
}

#[test]
fn private_source_owner_continuation_and_trace_release_only_the_approved_effect() {
    let policy = compile(include_str!("fixtures/provider-call-policy.rs")).unwrap();
    let mut ctx = context();
    ctx.amount_units = 1000;
    ctx.provider_call_input.as_mut().unwrap().input_key = "canonical-service-input".into();
    let (paused, trace) = allowit_sdk::evaluate_with_trace(&policy, &ctx);
    assert_eq!(paused.outcome, "awaiting_input");
    assert!(paused.system_operations.is_empty());
    assert!(!trace.unwrap().complete);
    let key = paused.input_key.unwrap();
    ctx.answers.insert(key.clone(), false);
    assert!(
        evaluate(&policy, Profile::Oracle, &ctx)
            .system_operations
            .is_empty()
    );
    ctx.answers.insert(key, true);
    let (approved, trace) = allowit_sdk::evaluate_with_trace(&policy, &ctx);
    assert_eq!(approved.outcome, "pass");
    assert_eq!(approved.system_operations.len(), 1);
    assert!(trace.unwrap().complete);
    ctx.amount_units = 1001;
    let refused = evaluate(&policy, Profile::Oracle, &ctx);
    assert_eq!(refused.outcome, "fail");
    assert!(refused.system_operations.is_empty());
}

mod source_facade {
    extern crate allowit_sdk as allowit;
    include!("fixtures/provider-call-policy.rs");
    #[test]
    fn provider_source_typechecks_and_direct_facade_cannot_admit_effects() {
        let ctx = Context::default();
        let params = new();
        let _future = _execute(&ctx, &params);
        assert!(!paysh::call(
            "air-quality",
            "canonical-service-input",
            1000,
            5000000,
            100000
        ));
    }
}

#[test]
fn new_operations_cannot_claim_legacy_registry_metadata() {
    let mut policy = compile(&source(&format!("let admitted = {CALL}; Ok(())"))).unwrap();
    policy.registry_version = "1.2.0".into();
    let rejected = evaluate(&policy, Profile::Oracle, &context());
    assert_eq!(rejected.code, "INVALID_ARTIFACT");
    assert!(rejected.system_operations.is_empty());
}
