use allowit_sdk::{
    PolicyEngine, StellarRail, compile_stellar_deposit_policy, compile_stellar_policy,
    select_stellar_engine,
};
fn source(body: &str) -> String {
    format!("use allowit::prelude::*; async fn _execute(ctx:&Context)->PolicyResult{{{body}}}")
}
#[test]
fn prefix_is_exact_ordered_subset_and_custom_suffix_stays_offchain() {
    let p=compile_stellar_policy(&source("allowit::recipients(grant::recipients)?; allowit::spent_cap(grant::spent_cap)?; if ctx.amount_units>0 {allowit::tokens(grant::tokens)?;} Ok(())")).unwrap();
    assert_eq!(p.source_profile.native_prefix, ["recipients", "spent_cap"]);
    assert!(p.source_profile.offchain_required);
    assert!(p.rust_source.contains("allowit_policy_rules::recipients"));
    assert!(!p.rust_source.contains("allowit_policy_rules::tokens"));
    assert_eq!(
        p.source_profile
            .functions
            .iter()
            .map(|f| f.location.as_str())
            .collect::<Vec<_>>(),
        ["standard_preflight", "standard_preflight", "offchain"]
    );
}
#[test]
fn pure_prefix_can_be_entirely_native_without_custom_code() {
    let s = source(
        "allowit::spent_cap(grant::spent_cap)?; allowit::spent_cap(grant::spent_cap)?; Ok(())",
    );
    let p = compile_stellar_policy(&s).unwrap();
    assert!(!p.source_profile.offchain_required);
    assert!(
        select_stellar_engine(
            &s,
            StellarRail::PolicyWallet,
            PolicyEngine::CommonPolicyEngine,
            true
        )
        .is_ok()
    );
    assert!(
        select_stellar_engine(
            &s,
            StellarRail::PolicyWallet,
            PolicyEngine::SimplePolicyEngine,
            false
        )
        .is_err()
    );
}
#[test]
fn literal_or_conditional_calls_are_never_hoisted() {
    for body in [
        "allowit::transaction_cap(100)?; allowit::spent_cap(grant::spent_cap)?; Ok(())",
        "if true {allowit::spent_cap(grant::spent_cap)?;} Ok(())",
    ] {
        let p = compile_stellar_policy(&source(body)).unwrap();
        assert!(p.source_profile.native_prefix.is_empty());
        assert!(p.source_profile.offchain_required);
        assert!(!p.rust_source.contains("allowit_policy_rules::spent_cap"));
    }
}
#[test]
fn deposit_has_exact_separate_schema_and_effect_manifest() {
    let s = source("allowit::recipients(grant::recipients)?; Ok(())");
    let transfer = compile_stellar_policy(&s).unwrap();
    let deposit = compile_stellar_deposit_policy(&s).unwrap();
    assert_ne!(transfer.argument_schema_hash, deposit.argument_schema_hash);
    assert_eq!(transfer.source_hash, deposit.source_hash);
    assert_eq!(
        deposit.source_profile.admitted_operations,
        ["stellar::sep56_deposit"]
    );
    assert!(
        deposit
            .source_profile
            .permitted_effects
            .contains(&"vault_share_credit".into())
    );
}
#[test]
fn whole_source_hash_changes_but_prefix_does_not_for_custom_suffix_update() {
    let a = compile_stellar_policy(&source("allowit::expiry(grant::expiry)?; Ok(())")).unwrap();
    let b=compile_stellar_policy(&source("allowit::expiry(grant::expiry)?; if ctx.amount_units==0 {return Err(PolicyError{code:\"ZERO\".into(),reason:\"Positive amount required.\".into()});} Ok(())")).unwrap();
    assert_ne!(a.source_hash, b.source_hash);
    assert_eq!(
        a.source_profile.native_prefix,
        b.source_profile.native_prefix
    );
    assert_eq!(a.argument_schema_hash, b.argument_schema_hash);
}
