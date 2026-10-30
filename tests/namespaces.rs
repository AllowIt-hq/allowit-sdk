use allowit_sdk::{
    Context, PolicyRuleFacts, PolicyScalar, Profile, TrustedPolicyState, compile, evaluate,
    evaluate_with_policy_rules, registry,
};
fn source(body: &str) -> String {
    format!("use allowit::prelude::*; async fn _execute(ctx:&Context)->PolicyResult {{ {body} }}")
}
fn context() -> Context {
    let mut c: Context = serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    c.policy_state = Some(TrustedPolicyState::default());
    c.rule_facts = Some(PolicyRuleFacts {
        operation_id: "namespace-op".into(),
        grant_available: true,
        amount_units: 10,
        asset_id: "SAC".into(),
        recipient: "vault".into(),
        now: 100,
        expense: None,
    });
    c
}
#[test]
fn canonical_rules_require_explicit_namespace_and_one_matching_grant_reference() {
    for name in [
        "spent_cap",
        "day_spent_cap",
        "day_total_cap",
        "transaction_cap",
        "call_cap",
        "recipients",
        "expiry",
        "tokens",
    ] {
        let p = compile(&source(&format!("allowit::{name}(grant::{name})?; Ok(())"))).unwrap();
        assert_eq!(p.native_prefix, [name]);
        assert!(compile(&source(&format!("{name}(grant::{name})?; Ok(())"))).is_err());
        let entry = registry()
            .into_iter()
            .find(|f| f.name == format!("allowit::{name}"))
            .unwrap();
        assert!(entry.signature.starts_with(&format!("allowit::{name}(")));
    }
    let p = compile(&source(
        "allowit::transaction_cap(grant::transaction_cap)?; Ok(())",
    ))
    .unwrap();
    let mut c = context();
    c.policy_state
        .as_mut()
        .unwrap()
        .grant
        .insert("transaction_cap".into(), PolicyScalar::U64(10));
    assert_eq!(evaluate_with_policy_rules(&p, &c).outcome, "pass");
    c.rule_facts.as_mut().unwrap().amount_units = 11;
    assert_eq!(
        evaluate_with_policy_rules(&p, &c).code,
        "TRANSACTION_CAP_EXCEEDED"
    );
}
#[test]
fn qualified_preferences_keep_evidence_and_owner_input() {
    let p = compile(&source(
        "allowit::v1::prelude::check_preference(ctx, \"Research?\", None, None).await?; Ok(())",
    ))
    .unwrap();
    assert_eq!(
        evaluate(&p, Profile::Oracle, &context()).outcome,
        "awaiting_input"
    );
    let p=compile(&source("allowit::v1::prelude::check_preference(ctx, \"Research?\", auto(\"deny\"), auto(\"approve\")).await?; Ok(())")).unwrap();
    assert_eq!(
        evaluate(&p, Profile::Contract, &context()).code,
        "SEMANTIC_EVIDENCE_REQUIRED"
    );
}
#[test]
fn exact_registry_rejects_other_paths_and_removed_spellings() {
    for name in [
        "paysh::transaction_cap",
        "stripe::transaction_cap",
        "jev::semantic",
        "allowit::v1::transaction_cap",
        "::allowit::transaction_cap",
        "allowit::transaction_cap::<u64>",
        "<Context as Vendor>::transaction_cap",
        "allowit::total_cap",
        "allowit::daily_cap",
        "allowit::recipient",
        "allowit::token",
        "allowit::set_cap_per_day",
        "allowit::cap_per_transaction",
        "allowit::amount_at_most",
        "allowit::require_recipient",
        "allowit::require_asset",
        "allowit::allow_actions",
        "allowit::fail",
        "allowit::state_remove",
        "allowit::cap_purchase_tiers",
    ] {
        assert!(
            compile(&source(&format!("{name}(100)?; Ok(())"))).is_err(),
            "{name}"
        );
    }
    assert!(compile(&source("let check=allowit::transaction_cap; Ok(())")).is_err());
}
#[test]
fn conditional_rules_remain_offchain_and_do_not_activate_a_later_prefix() {
    let p = compile(&source(
        "if true {allowit::transaction_cap(100)?;} allowit::recipients(grant::recipients)?; Ok(())",
    ))
    .unwrap();
    assert!(p.native_prefix.is_empty());
    assert!(p.offchain_required);
    assert_eq!(evaluate_with_policy_rules(&p, &context()).outcome, "pass");
}
mod typecheck {
    use allowit::prelude::*;
    use allowit_sdk as allowit;
    pub async fn check(ctx: &Context) -> PolicyResult {
        allowit::transaction_cap(grant::transaction_cap)?;
        allowit::recipients(grant::recipients)?;
        allowit::check_preference(
            allowit::preference_evidence(ctx, "Research?"),
            "Research?",
            None,
            None,
        )
        .await?;
        Ok(())
    }
}
#[test]
fn registered_names_type_check_in_the_native_facade() {
    let _ = typecheck::check;
}
#[test]
fn current_registry_does_not_recompile_retired_source_or_unknown_artifacts() {
    let mut p = compile(&source(
        "allowit::transaction_cap(grant::transaction_cap)?; Ok(())",
    ))
    .unwrap();
    assert_eq!(allowit_sdk::REGISTRY_VERSION, "1.8.0");
    assert_eq!(p.registry_version, "1.8.0");
    for version in ["1.5.0", "1.6.0", "1.7.0", "9.0.0"] {
        p.registry_version = version.into();
        assert_eq!(
            evaluate(&p, Profile::Oracle, &context()).code,
            "INVALID_ARTIFACT"
        );
    }
    // Preserve the historical fixture bytes. They have no fresh compiler route.
    assert!(compile(include_str!("fixtures/registry-stable-policy.rs")).is_err());
}
#[test]
fn namespace_spacing_preserves_exact_path_resolution() {
    for call in [
        "allowit :: transaction_cap",
        "allowit::/* comment */transaction_cap",
    ] {
        assert!(compile(&source(&format!("{call}(grant::transaction_cap)?; Ok(())"))).is_ok());
    }
    for call in [
        "r#allowit::transaction_cap",
        "allowit::r#transaction_cap",
        "allowit::allowit::transaction_cap",
    ] {
        assert!(compile(&source(&format!("{call}(grant::transaction_cap)?; Ok(())"))).is_err());
    }
}
#[test]
fn retired_vendors_have_no_execution_aliases() {
    assert!(
        registry()
            .iter()
            .all(|f| !f.name.starts_with("paysh::") && !f.name.starts_with("jev::"))
    );
}
