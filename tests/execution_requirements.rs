use allowit_sdk::{ExecutionFeature as F, compile};

fn policy(body: &str) -> String {
    format!("pub async fn evaluate(ctx: &Context) -> PolicyResult {{  {body} }}")
}

#[test]
fn lowered_preferences_reflect_actual_dependencies() {
    let ask = compile(&policy(
        "allowit::v1::prelude::check_preference(ctx, \"Research?\", None, None).await?; Ok(())",
    ))
    .unwrap();
    assert_eq!(ask.execution_requirements.features, vec![F::OwnerInput]);
    let scored = compile(&policy(
        "allowit::v1::prelude::check_preference(ctx, \"Research?\", 0.4, 0.85).await?; Ok(())",
    ))
    .unwrap();
    assert_eq!(
        scored.execution_requirements.features,
        vec![F::OwnerInput, F::SemanticEvidence]
    );
    assert_eq!(scored.execution_requirements.version, 1);
}

#[test]
fn all_branches_conditions_and_returns_contribute() {
    let compiled = compile(&policy(
        "\n        if allowit::context_u64(ctx, \"z\")? > 0 {\n            let score = allowit::semantic(ctx, \"Purpose?\")?;\n            if score.lower_bps > 8500 { return Ok(()); }\n        } else { allowit::require_user_input(ctx, \"Continue?\").await?; }\n        let value = allowit::context_u64(ctx, \"a\")?;\n        let repeated = allowit::context_u64(ctx, \"z\")?;\n        Ok(())\n    ",
    ))
    .unwrap();
    let req = compiled.execution_requirements;
    assert_eq!(
        req.features,
        vec![F::OwnerInput, F::RuntimeContextU64, F::SemanticEvidence]
    );
    assert_eq!(req.context_u64_keys, vec!["a", "z"]);
    assert!(!req.dynamic_context_keys);
}

#[test]
fn dynamic_keys_are_explicit_and_never_guessed() {
    let req = compile(&policy(
        "let key = \"yield\"; let value = allowit::context_u64(ctx, key)?; Ok(())",
    ))
    .unwrap()
    .execution_requirements;
    assert!(req.dynamic_context_keys);
    assert!(req.context_u64_keys.is_empty());
    assert_eq!(req.features, vec![F::RuntimeContextU64]);
}

#[test]
fn exact_checks_do_not_become_semantic_classifications() {
    let req = compile(&policy(
        r#"allowit::recipients(&["exact-id"])?; allowit::transaction_cap(10_000_000)?; Ok(())"#,
    ))
    .unwrap()
    .execution_requirements;
    assert_eq!(req.features, vec![F::PolicyRules]);
}

#[test]
fn evidence_does_not_create_a_retired_purchase_history_rule() {
    let req = compile(&policy(
        "let evidence=allowit::confidence(ctx, \"source\")?; Ok(())",
    ))
    .unwrap()
    .execution_requirements;
    assert_eq!(req.features, vec![F::ConfidenceEvidence]);
    assert!(
        compile(&policy(
            "allowit::cap_purchase_tiers(ctx, \"20\", 2, \"USDC\")?; Ok(())"
        ))
        .is_err()
    );
}

#[test]
fn serialization_and_parameter_edits_preserve_contract() {
    let original = compile(&policy(
        "allowit::v1::prelude::check_preference(ctx, \"Research?\", 0.4, 0.85).await?; Ok(())",
    ))
    .unwrap();
    let changed = compile(&original.source.replace("0.85", "0.9")).unwrap();
    assert_eq!(
        original.execution_requirements,
        changed.execution_requirements
    );
    assert_ne!(original.source_hash, changed.source_hash);
    assert_ne!(original.ir_hash, changed.ir_hash);
    let value = serde_json::to_value(&original).unwrap();
    assert_eq!(
        value["execution_requirements"]["features"],
        serde_json::json!(["owner_input", "semantic_evidence"])
    );
}

#[test]
fn requirements_cannot_be_forged_under_a_valid_source_hash() {
    let mut compiled = compile(&policy(
        "allowit::v1::prelude::check_preference(ctx, \"Research?\", 0.4, 0.85).await?; Ok(())",
    ))
    .unwrap();
    compiled.execution_requirements.features.clear();
    let context = serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    assert_eq!(
        allowit_sdk::evaluate(&compiled, allowit_sdk::Profile::Oracle, &context).code,
        "INVALID_ARTIFACT"
    );
}

#[test]
fn requirements_are_not_a_reachability_or_permission_claim() {
    let compiled = compile(&policy(
        "return Ok(()); allowit::require_user_input(ctx, \"Unreachable\").await?; Ok(())",
    ))
    .unwrap();
    assert_eq!(
        compiled.execution_requirements.features,
        vec![F::OwnerInput]
    );
}
