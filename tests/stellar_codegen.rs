use allowit_sdk::{StellarPolicyContext, compile_stellar_policy};
fn source(body: &str) -> String {
    format!(
        "use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult {{ {body} }}"
    )
}
fn context() -> StellarPolicyContext {
    StellarPolicyContext {
        asset: "SAC".into(),
        recipient: "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".into(),
        token: "USDC".into(),
        decimals: 7,
        amount: 20_000_000,
        spent_daily: 30_000_000,
        spent_total: 30_000_000,
        calls: 0,
        now: 86401,
        spent_day: 1,
    }
}
#[test]
fn deposit_profile_binds_schema_and_actual_action_without_transfer_substitution() {
    let policy = allowit_sdk::compile_stellar_deposit_policy(&source(
        r#"allowit::allow_actions(ctx, &["stellar::sep56_deposit"])?; Ok(())"#,
    ))
    .unwrap();
    assert!(policy.evaluate_operation(&context(), "stellar::sep56_deposit"));
    assert!(!policy.evaluate(&context()));
    assert_eq!(
        policy.source_profile.admitted_operations,
        vec!["stellar::sep56_deposit"]
    );
    assert!(
        policy
            .rust_source
            .contains("ctx.operation==soroban_sdk::String::from_str")
    );
    assert!(
        policy
            .rust_source
            .contains("ADMITTED_OPERATION:&str=\"stellar::sep56_deposit\"")
    );
    assert_eq!(policy.argument_schema_hash, {
        use sha2::{Digest, Sha256};
        <[u8; 32]>::from(Sha256::digest(
            allowit_sdk::stellar_vault::DEPOSIT_ARGUMENT_SCHEMA,
        ))
    });
    let denied = allowit_sdk::compile_stellar_deposit_policy(&source(
        r#"allowit::allow_actions(ctx, &["stellar::sep41_transfer"])?; Ok(())"#,
    ))
    .unwrap();
    assert!(!denied.evaluate_operation(&context(), "stellar::sep56_deposit"));
}
#[test]
fn authored_destination_allowlist_selects_vaults_without_hidden_configuration() {
    let a = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";
    let b = "CBNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVXQOK";
    // A second distinct contract address with a valid checksum.
    let source = source(&format!(
        r#"if !allowit::is_one_of(&ctx.recipient, &["{a}", "{b}"])? {{ return allowit::fail("vault not allowed"); }} Ok(())"#
    ));
    let policy = allowit_sdk::compile_stellar_deposit_policy(&source).unwrap();
    let mut c = context();
    for vault in [a, b] {
        c.recipient = vault.into();
        assert!(policy.evaluate_operation(&c, "stellar::sep56_deposit"));
    }
    c.recipient = context().recipient;
    assert!(!policy.evaluate_operation(&c, "stellar::sep56_deposit"));
}
#[test]
fn monetary_guards_lower_exactly_to_native_units() {
    let policy=compile_stellar_policy(&source(r#"allowit::cap_per_transaction(ctx, "2", "USDC")?; allowit::set_cap_per_day(ctx, "5", "USDC")?; Ok(())"#)).unwrap();
    let mut c = context();
    assert!(policy.evaluate(&c));
    c.amount += 1;
    assert!(!policy.evaluate(&c));
    c.amount = 20_000_000;
    c.spent_daily += 1;
    c.spent_total += 1;
    assert!(!policy.evaluate(&c));
    c.now += 86400;
    assert!(policy.evaluate(&c));
    c.now = 1;
    assert!(!policy.evaluate(&c));
    c = context();
    c.token = "XLM".into();
    assert!(!policy.evaluate(&c));
    c = context();
    c.decimals = 6;
    assert!(!policy.evaluate(&c));
    assert!(policy.rust_source.contains("fn _execute("));
    assert!(policy.rust_source.contains("20000000i128"));
    assert!(!policy.rust_source.contains("serde"));
    assert!(!policy.rust_source.contains("Program"));
}
#[test]
fn private_policy_preserves_branching_and_recipient() {
    let policy=compile_stellar_policy(&source(r#"allowit::require_recipient(ctx, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF")?; if !allowit::amount_at_most(ctx, "2")? { return allowit::fail("too much"); } Ok(())"#)).unwrap();
    let mut c = context();
    assert!(policy.evaluate(&c));
    c.amount += 1;
    assert!(!policy.evaluate(&c));
    c = context();
    c.recipient = "another address".into();
    assert!(!policy.evaluate(&c));
}
#[test]
fn hoisted_configuration_cannot_be_skipped_by_early_return() {
    let policy = compile_stellar_policy(&source(
        r#"return Ok(()); allowit::set_cap_per_day(ctx, "1", "USDC")?;"#,
    ))
    .unwrap();
    assert!(!policy.evaluate(&context()));
    let policy = compile_stellar_policy(&source(
        r#"return Ok(()); allowit::require_asset(ctx, "USDC")?;"#,
    ))
    .unwrap();
    let mut c = context();
    c.token = "XLM".into();
    assert!(!policy.evaluate(&c));
}
#[test]
fn unsupported_features_and_raw_six_decimal_fields_fail_admission() {
    for body in [
        r#"if ctx.amount_units > 10 { return allowit::fail("raw units"); } Ok(())"#,
        r#"if false { let x=ctx.amount_units; } Ok(())"#,
        r#"allowit::require_user_input(ctx, "approve").await?; Ok(())"#,
        r#"allowit::require_merchant(ctx, "merchant")?; Ok(())"#,
    ] {
        assert!(compile_stellar_policy(&source(body)).is_err(), "{body}");
    }
    assert!(
        compile_stellar_policy("pub async fn execute(ctx: &Context)->PolicyResult { Ok(()) }")
            .is_err()
    );
}
#[test]
fn exact_source_digest_and_schema_are_bound() {
    let a = compile_stellar_policy(&source("Ok(())")).unwrap();
    let b = compile_stellar_policy(&source(" Ok(())")).unwrap();
    assert_ne!(a.source_hash, b.source_hash);
    assert_eq!(a.argument_schema_hash, b.argument_schema_hash);
    let denied = compile_stellar_policy(&source(r#"allowit::fail("blocked")"#)).unwrap();
    assert!(!denied.evaluate(&context()));
}

#[test]
fn explicit_integer_micro_units_match_readable_monetary_source() {
    let readable=compile_stellar_policy(&source(r#"allowit::cap_per_transaction(ctx, "2", "USDC")?; allowit::set_cap_per_day(ctx, "5", "USDC")?; Ok(())"#)).unwrap();
    let explicit =
        compile_stellar_policy(include_str!("fixtures/generated-small-payments.rs")).unwrap();
    for amount in [1, 19_999_999, 20_000_000, 20_000_001, i128::MAX] {
        for spent in [0, 29_999_999, 30_000_000, 30_000_001, i128::MAX] {
            let mut c = context();
            c.amount = amount;
            c.spent_daily = spent;
            c.spent_total = spent;
            assert_eq!(
                readable.evaluate(&c),
                explicit.evaluate(&c),
                "amount={amount} spent={spent}"
            );
        }
    }
}
#[test]
fn native_request_units_are_never_rounded_and_unsupported_xlm_helpers_reject() {
    let p = compile_stellar_policy(&source(
        r#"allowit::cap_per_transaction(ctx, "0.000001", "USDC")?; Ok(())"#,
    ))
    .unwrap();
    let mut c = context();
    c.amount = 10;
    c.spent_daily = 0;
    c.spent_total = 0;
    assert!(p.evaluate(&c));
    c.amount = 11;
    assert!(!p.evaluate(&c));
    assert!(
        compile_stellar_policy(&source(
            r#"allowit::cap_per_transaction(ctx, "2", "XLM")?; Ok(())"#
        ))
        .is_err()
    );
    assert!(
        compile_stellar_policy(&source(
            r#"if ctx.amount_units <= 2000000 { return Ok(()); } allowit::fail("raw")"#
        ))
        .is_err()
    );
}

fn core_pass(source: &str, native: &StellarPolicyContext) -> bool {
    let compiled = allowit_sdk::compile(source).unwrap();
    let mut c: allowit_sdk::Context =
        serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    c.amount_units = u64::try_from(native.amount / 10).unwrap();
    c.spent_units = u64::try_from(native.spent_total / 10).unwrap();
    c.daily_spent_units = Some(u64::try_from(native.spent_daily / 10).unwrap());
    c.spent_day = Some(native.spent_day);
    c.now = native.now;
    c.token = native.token.clone();
    c.recipient = native.recipient.clone();
    allowit_sdk::evaluate(&compiled, allowit_sdk::Profile::Contract, &c).outcome == "pass"
}
#[test]
fn unused_fallible_unit_bindings_still_propagate_errors() {
    for body in [
        r#"let checked = allowit::cap_per_transaction(ctx, "1", "USDC")?; Ok(())"#,
        r#"let checked = allowit::require_recipient(ctx, "CBNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVXQOK")?; Ok(())"#,
        r#"let checked = allowit::require_asset(ctx, "XLM")?; Ok(())"#,
    ] {
        let source = source(body);
        assert!(allowit_sdk::compile(&source).is_err());
        assert!(compile_stellar_policy(&source).is_err());
    }
}
#[test]
fn false_predicates_are_values_and_unused_results_do_not_reject() {
    for body in [
        r#"let fits = allowit::amount_at_most(ctx, "1")?; Ok(())"#,
        r#"let fits = false; Ok(())"#,
    ] {
        let source = source(body);
        let p = compile_stellar_policy(&source).unwrap();
        let c = context();
        assert!(core_pass(&source, &c), "{body}");
        assert!(p.evaluate(&c), "{body}");
    }
}
#[test]
fn boolean_not_and_or_never_convert_errors_into_approval() {
    for body in [
        r#"if !allowit::amount_at_most(ctx, "1")? { return Ok(()); } allowit::fail("small")"#,
        r#"if allowit::amount_at_most(ctx, "1")? || true { return Ok(()); } allowit::fail("small")"#,
    ] {
        let source = source(body);
        let p = compile_stellar_policy(&source).unwrap();
        let mut c = context();
        c.token = "XLM".into();
        assert!(!p.evaluate(&c), "{body}");
    }
    let p=compile_stellar_policy(&source(r#"if true || allowit::amount_at_most(ctx, "1")? { return Ok(()); } allowit::fail("small")"#)).unwrap();
    let mut c = context();
    c.token = "XLM".into();
    assert!(p.evaluate(&c));
}
#[test]
fn unit_values_cannot_be_conditions_and_strkey_checksum_is_checked() {
    let discarded = source(r#"allowit::amount_at_most(ctx, "1")?; Ok(())"#);
    assert!(allowit_sdk::compile(&discarded).is_err());
    assert!(compile_stellar_policy(&discarded).is_err());
    assert!(
        compile_stellar_policy(&source(
            r#"if !allowit::require_asset(ctx, "USDC")? { return Ok(()); } Ok(())"#
        ))
        .is_err()
    );
    assert!(compile_stellar_policy(&source(r#"allowit::require_recipient(ctx, "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHA")?; Ok(())"#)).is_err());
}

#[test]
fn source_profile_declares_only_the_actual_native_wrapper_effect() {
    let source = source("Ok(())");
    let policy = compile_stellar_policy(&source).unwrap();
    assert_eq!(policy.source_profile.profile, "allowit-stellar-transfer-v1");
    assert_eq!(policy.source_profile.version, 1);
    assert_eq!(policy.source_profile.source_hash, policy.source_hash);
    assert_eq!(policy.source_profile.public_entrypoint, "execute");
    assert_eq!(policy.source_profile.private_entrypoint, "_execute");
    assert_eq!(
        policy.source_profile.admitted_operations,
        vec!["stellar::sep41_transfer"]
    );
    assert_eq!(
        policy.source_profile.permitted_effects,
        vec!["bound_asset_debit", "recipient_credit"]
    );
    assert_eq!(
        policy.source_profile.argument_schema_digest,
        policy.argument_schema_hash
    );
}

#[test]
fn native_effect_is_catalogued_without_generic_callable_admission() {
    let entry = allowit_sdk::registry()
        .into_iter()
        .find(|v| v.name == "stellar::sep41_transfer")
        .unwrap();
    assert_eq!(entry.effect, "native_effect");
    assert_eq!(
        entry.signature,
        "stellar::sep41_transfer(token: AccountId, from: AccountId, to: AccountId, amount: i128) -> Result<TransferReceipt>"
    );
    assert!(entry.typed_signature.is_none());
    assert!(allowit_sdk::compile_stellar_policy("use allowit::v1::prelude::*; async fn _execute(ctx:&Context)->PolicyResult { stellar::sep41_transfer(ctx)?; Ok(()) }").is_err());
}

fn functional(body: &str) -> String {
    format!(
        "use allowit::v1::prelude::*; async fn _execute(request:&Context,grant:&Grant,state:&State)->Result<Spent,PolicyError>{{{body}}}"
    )
}
#[test]
fn functional_properties_use_exact_scalars_and_checked_arithmetic() {
    use allowit_sdk::{PolicyScalar as V, ScalarType, StellarEvaluation};
    use std::collections::BTreeMap;
    let p=compile_stellar_policy(&functional(r#"let initial=allowit::spent(state)?;let n=allowit::state_get(&initial,"payments",9007199254740993u64)?;let enabled=allowit::state_get(&initial,"enabled",true)?;let label=allowit::state_get(&initial,"label","initial")?;if !enabled||label!="initial"{return allowit::fail("Denied");}let a=allowit::state_set(initial,"payments",n+1)?;let b=allowit::state_set(a,"enabled",false)?;let output=allowit::state_set(b,"label","updated")?;Ok(output)"#)).unwrap();
    assert_eq!(
        p.property_keys
            .iter()
            .map(|k| (&*k.name, k.kind))
            .collect::<Vec<_>>(),
        vec![
            ("payments", ScalarType::U64),
            ("enabled", ScalarType::Bool),
            ("label", ScalarType::String)
        ]
    );
    let mut values = BTreeMap::from([("opaque".into(), V::Unsupported)]);
    assert_eq!(
        p.evaluation_with_state(&context(), &mut values, &BTreeMap::new()),
        StellarEvaluation::Approved
    );
    assert_eq!(values["payments"], V::U64(9007199254740994));
    assert_eq!(values["enabled"], V::Bool(false));
    assert_eq!(values["label"], V::String("updated".into()));
    assert_eq!(values["opaque"], V::Unsupported);
    let before = values.clone();
    assert!(!p.evaluate_with_state(&context(), &mut values, &BTreeMap::new()));
    assert_eq!(values, before);
    let max=compile_stellar_policy(&functional(r#"let initial=allowit::spent(state)?;let n=allowit::state_get(&initial,"n",18446744073709551615u64)?;let output=allowit::state_set(initial,"n",n+1)?;Ok(output)"#)).unwrap();
    let mut values = BTreeMap::new();
    assert!(!max.evaluate_with_state(&context(), &mut values, &BTreeMap::new()));
    assert!(values.is_empty());
    assert!(max.rust_source.contains("checked_add"));
}
#[test]
fn only_returned_map_commits_and_with_spent_replaces_without_merge() {
    use allowit_sdk::PolicyScalar as V;
    use std::collections::BTreeMap;
    let p=compile_stellar_policy(&functional(r#"let earlier=allowit::spent(state)?;let working=allowit::spent(state)?;let later=allowit::state_set(working,"n",8u64)?;Ok(earlier)"#)).unwrap();
    let mut values = BTreeMap::from([("n".into(), V::U64(4))]);
    assert!(p.evaluate_with_state(&context(), &mut values, &BTreeMap::new()));
    assert_eq!(values["n"], V::U64(4));
    let p=compile_stellar_policy(&functional(r#"let initial=allowit::spent(state)?;let changed=allowit::state_set(initial,"n",8u64)?;let after=allowit::with_spent(state,changed)?;let output=allowit::spent(&after)?;Ok(output)"#)).unwrap();
    assert!(p.evaluate_with_state(&context(), &mut values, &BTreeMap::new()));
    assert_eq!(values["n"], V::U64(8));
}
#[test]
fn typed_reads_follow_executed_branches_and_preserve_unrelated_values() {
    use allowit_sdk::PolicyScalar as V;
    use std::collections::BTreeMap;
    let p=compile_stellar_policy(&functional(r#"let initial=allowit::spent(state)?;if false{let ignored=allowit::state_get(&initial,"unreachable",0u64)?;}let removed=allowit::state_remove(initial,"old")?;let value=allowit::state_get(&removed,"old","new")?;if value!="new"{return allowit::fail("Removal failed");}Ok(removed)"#)).unwrap();
    let mut values = BTreeMap::from([
        ("unreachable".into(), V::Unsupported),
        ("old".into(), V::U64(5)),
    ]);
    assert!(p.evaluate_with_state(&context(), &mut values, &BTreeMap::new()));
    assert!(!values.contains_key("old"));
    assert_eq!(values["unreachable"], V::Unsupported);
    let read=compile_stellar_policy(&functional(r#"let initial=allowit::spent(state)?;let n=allowit::state_get(&initial,"unreachable",0u64)?;Ok(initial)"#)).unwrap();
    assert!(!read.evaluate_with_state(&context(), &mut values, &BTreeMap::new()));
    for bad in [
        r#"let initial=allowit::spent(state)?;let key="dynamic";let n=allowit::state_get(&initial,key,0u64)?;Ok(initial)"#,
        r#"let state=0u64;allowit::spent(state)"#,
        r#"Ok(())"#,
    ] {
        assert!(compile_stellar_policy(&functional(bad)).is_err());
    }
}
#[test]
fn owner_grant_reads_are_typed_and_do_not_activate_unrelated_keys() {
    use allowit_sdk::PolicyScalar as V;
    use std::collections::BTreeMap;
    let p=compile_stellar_policy(&functional(r#"let initial=allowit::spent(state)?;let n=allowit::state_get(&initial,"n",0u64)?;let limit=allowit::grant_get(grant,"custom_limit",3u64)?;if n>=limit{return allowit::fail("Count exceeded");}let output=allowit::state_set(initial,"n",n+1)?;Ok(output)"#)).unwrap();
    let mut values = BTreeMap::new();
    let grant = BTreeMap::from([
        ("custom_limit".into(), V::U64(1)),
        ("unrelated".into(), V::Unsupported),
    ]);
    assert!(p.evaluate_with_state(&context(), &mut values, &grant));
    assert!(!p.evaluate_with_state(&context(), &mut values, &grant));
    assert!(p.rust_source.contains("grant.get"));
    assert!(!p.rust_source.contains("ctx.grant"));
    let bad = BTreeMap::from([("custom_limit".into(), V::Unsupported)]);
    assert!(!p.evaluate_with_state(&context(), &mut values, &bad));
}
#[test]
fn generic_interpreter_accepts_only_trusted_state_and_only_final_map() {
    use allowit_sdk::{PolicyScalar as V, TrustedPolicyState};
    let source = functional(
        r#"let initial=allowit::spent(state)?;let n=allowit::state_get(&initial,"n",0u64)?;if n>0{return allowit::fail("Denied");}let output=allowit::state_set(initial,"n",n+1)?;Ok(output)"#,
    );
    let p = allowit_sdk::compile(&source).unwrap();
    let mut c: allowit_sdk::Context =
        serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    assert_eq!(
        allowit_sdk::evaluate(&p, allowit_sdk::Profile::Oracle, &c).code,
        "POLICY_STATE_REQUIRED"
    );
    let mut json = serde_json::to_value(&c).unwrap();
    json["policy_state"] = serde_json::json!({"properties":{"n":0}});
    assert!(serde_json::from_value::<allowit_sdk::Context>(json).is_err());
    c.policy_state = Some(TrustedPolicyState::default());
    assert_eq!(
        allowit_sdk::evaluate(&p, allowit_sdk::Profile::Oracle, &c).outcome,
        "pass"
    );
    assert_eq!(
        c.policy_state.as_ref().unwrap().properties.borrow()["n"],
        V::U64(1)
    );
    assert_eq!(
        allowit_sdk::evaluate(&p, allowit_sdk::Profile::Oracle, &c).outcome,
        "fail"
    );
    assert_eq!(
        c.policy_state.as_ref().unwrap().properties.borrow()["n"],
        V::U64(1)
    );
}
#[test]
fn explicit_engine_calls_require_native_authority_but_classic_guards_remain_local() {
    use allowit_sdk::StellarEvaluation;
    use std::collections::BTreeMap;
    for name in [
        "daily_cap",
        "transaction_cap",
        "total_cap",
        "call_cap",
        "recipient",
        "expiry",
        "token",
    ] {
        assert!(
            compile_stellar_policy(&functional(&format!("engine::{name}(request,state)"))).is_err()
        );
    }
    let p = compile_stellar_policy(&source(
        r#"allowit::cap_per_transaction(ctx,"3","USDC")?;Ok(())"#,
    ))
    .unwrap();
    assert!(p.evaluate_classic(&context()));
    assert!(p.requires_native_preflight());
    assert_eq!(
        p.evaluation_with_state(&context(), &mut BTreeMap::new(), &BTreeMap::new()),
        StellarEvaluation::RequiresNativeEngine
    );
    assert!(
        p.rust_source
            .contains("allowit_policy_rules::cap_per_transaction")
    );
    assert!(p.rust_source.contains("->Result<Spent,()>"));
    assert!(!p.rust_source.contains("PolicyStorage"));
}
#[test]
fn actual_mixed_and_local_authored_inputs_compile_without_reconstruction() {
    assert!(compile_stellar_policy(include_str!("fixtures/stellar-state-policy.rs")).is_err());
    let local =
        compile_stellar_policy(include_str!("fixtures/stellar-local-state-policy.rs")).unwrap();
    assert!(local.engine_checks.is_empty());
    assert!(!local.placement_manifest.possible_shared_spent_writer);
    assert!(local.requires_native_preflight());
    assert!(
        local
            .rust_source
            .contains("allowit_policy_rules::daily_cap")
    );
}
#[test]
fn registry_compatibility_is_feature_aware_without_rewriting_historical_inputs() {
    let old = source("Ok(())");
    let mut artifact = allowit_sdk::compile(&old).unwrap();
    let c = serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    artifact.registry_version = "1.6.0".into();
    assert_eq!(
        allowit_sdk::evaluate(&artifact, allowit_sdk::Profile::Oracle, &c).outcome,
        "pass"
    );
    artifact.registry_version = "9.9.0".into();
    assert_eq!(
        allowit_sdk::evaluate(&artifact, allowit_sdk::Profile::Oracle, &c).code,
        "INVALID_ARTIFACT"
    );
    let mut new = allowit_sdk::compile(&functional("allowit::spent(state)")).unwrap();
    assert_eq!(new.registry_version, "1.7.0");
    new.registry_version = "1.6.0".into();
    assert_eq!(
        allowit_sdk::evaluate(&new, allowit_sdk::Profile::Oracle, &c).code,
        "INVALID_ARTIFACT"
    );
}
#[allow(dead_code, unused_variables)]
mod functional_rust_facade {
    use allowit::v1::prelude::*;
    use allowit_sdk as allowit;
    async fn _execute(
        request: &Context,
        grant: &Grant,
        state: &State,
    ) -> Result<Spent, PolicyError> {
        let initial = allowit::spent(state)?;
        let n = allowit::state_get(&initial, "n", 0u64)?;
        let output = allowit::state_set(initial, "n", n + 1)?;
        Ok(output)
    }
    async fn engine_source(
        request: &Context,
        grant: &Grant,
        state: &State,
    ) -> Result<Spent, PolicyError> {
        let daily = allowit::daily_cap(request, state)?;
        let next = allowit::with_spent(state, daily)?;
        engine::total_cap(request, &next)
    }
}

#[test]
fn placement_rejects_hybrid_and_cost_selection_without_a_fallback() {
    use allowit_sdk::{RulePlacement, StellarCompileOptions, compile_stellar_policy_with_options};
    let authored = functional("allowit::daily_cap(request,state)");
    let local = compile_stellar_policy(&authored).unwrap();
    assert_eq!(
        local.placement_manifest.checks[0].selected,
        RulePlacement::Local
    );
    assert!(
        local
            .rust_source
            .contains("allowit_policy_rules::daily_cap")
    );
    for options in [
        StellarCompileOptions {
            default_placement: RulePlacement::Shared,
            ..Default::default()
        },
        StellarCompileOptions {
            expected_transactions: Some(100),
            ..Default::default()
        },
    ] {
        assert!(compile_stellar_policy_with_options(&authored, &options).is_err());
    }
    assert!(compile_stellar_policy(&functional("engine::daily_cap(request,state)")).is_err());
    assert!(compile_stellar_policy(&functional("self::confidence(request,state)")).is_err());
}

#[test]
fn native_return_ir_is_internal_and_never_leaks_into_source_registry_calls() {
    for body in [
        r#"allowit::fail("Denied")"#,
        r#"let refused=allowit::fail("Denied")?;Ok(refused)"#,
        r#"let initial=allowit::spent(state)?;Ok(initial)"#,
    ] {
        let source = functional(body);
        let policy = allowit_sdk::compile(&source).unwrap();
        assert!(
            policy
                .calls
                .iter()
                .all(|c| !c.name.starts_with("native_") && !c.name.starts_with("__native"))
        );
        assert!(compile_stellar_policy(&source).is_ok());
    }
}

#[test]
fn engine_selection_preserves_complete_control_flow_and_rail_boundaries() {
    use allowit_sdk::{
        PolicyEngine as E, StellarRail as R, compile_stellar_wallet, recommend_stellar_engines,
    };
    let pass = source("Ok(())");
    let common = functional(
        "let daily=allowit::daily_cap(request,state)?;let next=allowit::with_spent(state,daily)?;allowit::total_cap(request,&next)",
    );
    let custom = include_str!("fixtures/stellar-local-state-policy.rs");
    for (source, engine, rules) in [
        (&pass[..], E::SimplePolicyEngine, vec![]),
        (
            &common[..],
            E::CommonPolicyEngine,
            vec!["daily_cap", "total_cap"],
        ),
        (custom, E::CustomPolicy, vec![]),
    ] {
        let compiled =
            compile_stellar_wallet(source, "stellar:testnet", R::PolicyWallet, engine, true)
                .unwrap();
        assert_eq!(compiled.selection.rules, rules);
        assert_eq!(
            compiled.generated_source.is_some(),
            engine == E::CustomPolicy
        );
        assert!(compiled.selection.onchain_policy);
        assert_eq!(recommend_stellar_engines(source).unwrap()[0].engine, engine);
        assert!(
            compile_stellar_wallet(source, "solana:devnet", R::PolicyWallet, engine, true).is_err()
        );
        assert!(
            compile_stellar_wallet(source, "eip155:42431", R::PolicyWallet, engine, true).is_err()
        );
    }
    for body in [
        "let ignored=allowit::daily_cap(request,state)?;allowit::spent(state)",
        "if false{return allowit::daily_cap(request,state);}allowit::spent(state)",
        "let a=allowit::daily_cap(request,state)?;let b=allowit::total_cap(request,state)?;Ok(b)",
        "let a=allowit::daily_cap(request,state)?;let next=allowit::with_spent(state,a)?;allowit::daily_cap(request,&next)",
    ] {
        let source = functional(body);
        assert!(
            compile_stellar_wallet(
                &source,
                "stellar:testnet",
                R::PolicyWallet,
                E::CommonPolicyEngine,
                true
            )
            .is_err(),
            "{body}"
        );
        assert!(
            compile_stellar_wallet(
                &source,
                "stellar:testnet",
                R::PolicyWallet,
                E::SimplePolicyEngine,
                false
            )
            .is_err(),
            "{body}"
        );
        assert!(
            compile_stellar_wallet(
                &source,
                "stellar:testnet",
                R::PolicyWallet,
                E::CustomPolicy,
                true
            )
            .is_ok(),
            "{body}"
        );
    }
    let capped = source("allowit::cap_per_transaction(ctx,\"2\",\"USDC\")?;Ok(())");
    assert!(
        compile_stellar_wallet(
            &capped,
            "stellar:testnet",
            R::Classic,
            E::SimplePolicyEngine,
            false
        )
        .is_ok()
    );
    assert!(
        compile_stellar_wallet(
            &capped,
            "stellar:testnet",
            R::Classic,
            E::SimplePolicyEngine,
            true
        )
        .is_err()
    );
    for engine in [E::CommonPolicyEngine, E::CustomPolicy] {
        assert!(
            compile_stellar_wallet(&common, "stellar:testnet", R::Classic, engine, false).is_err()
        );
    }
    let compile =
        allowit_sdk::process_value(serde_json::json!({"operation":"compile","source":common}));
    assert_eq!(
        compile["stellar_engines"][0]["engine"],
        "common_policy_engine"
    );
    let selected = allowit_sdk::process_value(
        serde_json::json!({"operation":"compile_stellar_wallet","source":common,"network":"stellar:testnet","rail":"policy_wallet","engine":"common_policy_engine","require_onchain":true}),
    );
    assert_eq!(selected["ok"], true);
    assert_eq!(
        selected["compilation"]["selection"]["rules"],
        serde_json::json!(["daily_cap", "total_cap"])
    );
    let unsupported = source("allowit::require_user_input(ctx,\"approval\").await?;Ok(())");
    assert!(recommend_stellar_engines(&unsupported).is_err());
    let result =
        allowit_sdk::process_value(serde_json::json!({"operation":"compile","source":unsupported}));
    assert_eq!(result["stellar_engines"], serde_json::json!([]));
    assert!(result["stellar_engine_error"].is_object());
}
