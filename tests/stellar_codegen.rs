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
