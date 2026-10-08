use allowit_sdk::{ExecutionFeature as F, compile};

fn policy(body: &str) -> String {
    format!(
        "pub async fn evaluate(ctx: &Context) -> PolicyResult {{ set_cap(ctx, \"100\", \"USDC\")?; {body} }}"
    )
}

#[test]
fn lowered_preferences_reflect_actual_dependencies() {
    let ask = compile(&policy(
        "check_preference(ctx, \"Research?\", None, None).await?; Ok(())",
    ))
    .unwrap();
    assert_eq!(ask.execution_requirements.features, vec![F::OwnerInput]);
    let scored = compile(&policy(
        "check_preference(ctx, \"Research?\", 0.4, 0.85).await?; Ok(())",
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
        r#"
        if context_u64(ctx, "z")? > 0 {
            let score = semantic(ctx, "Purpose?")?;
            if score.lower_bps > 8500 { return Ok(()); }
        } else { require_user_input(ctx, "Continue?").await?; }
        let value = context_u64(ctx, "a")?;
        let repeated = context_u64(ctx, "z")?;
        Ok(())
    "#,
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
        r#"let key = "yield"; let value = context_u64(ctx, key)?; Ok(())"#,
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
        r#"require_merchant(ctx, "exact-id")?; cap_per_transaction(ctx, "10", "USDC")?; Ok(())"#,
    ))
    .unwrap()
    .execution_requirements;
    assert!(req.features.is_empty());
}

#[test]
fn evidence_and_authoritative_ledger_are_distinct() {
    let req = compile(&policy(r#"cap_purchase_tiers(ctx, "20", 2, "USDC")?; let evidence = confidence(ctx, "source")?; Ok(())"#)).unwrap().execution_requirements;
    assert_eq!(
        req.features,
        vec![F::ConfidenceEvidence, F::PurchaseHistory]
    );
}

#[test]
fn serialization_and_parameter_edits_preserve_contract() {
    let original = compile(&policy(
        "check_preference(ctx, \"Research?\", 0.4, 0.85).await?; Ok(())",
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
        "check_preference(ctx, \"Research?\", 0.4, 0.85).await?; Ok(())",
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
        "return Ok(()); require_user_input(ctx, \"Unreachable\").await?; Ok(())",
    ))
    .unwrap();
    assert_eq!(
        compiled.execution_requirements.features,
        vec![F::OwnerInput]
    );
}
