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
