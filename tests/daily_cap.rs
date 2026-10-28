use allowit_sdk::{Context, Profile, compile, evaluate};
fn context() -> Context {
    serde_json::from_str(include_str!("../examples/context.json")).unwrap()
}
#[test]
fn daily_accounting_is_required_resets_at_midnight_and_rejects_rollback() {
    let policy = compile("pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap_per_day(ctx, \"10\", \"USDC\")?; Ok(()) }").unwrap();
    assert!(policy.limit.is_empty());
    assert_eq!(policy.daily_limit, "10");
    let mut ctx = context();
    ctx.now = 86_399;
    ctx.spent_day = Some(0);
    ctx.daily_spent_units = Some(5_000_000);
    ctx.spent_units = 80_000_000;
    for profile in [Profile::Oracle, Profile::Contract] {
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "pass");
        ctx.daily_spent_units = Some(5_000_001);
        assert_eq!(evaluate(&policy, profile, &ctx).code, "DAILY_CAP_EXCEEDED");
        ctx.now = 86_400;
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "pass");
        ctx.spent_day = Some(2);
        assert_eq!(
            evaluate(&policy, profile, &ctx).code,
            "CLOCK_WENT_BACKWARDS"
        );
        ctx.spent_day = None;
        assert_eq!(
            evaluate(&policy, profile, &ctx).code,
            "DAILY_ACCOUNTING_REQUIRED"
        );
        ctx.spent_day = Some(0);
        ctx.daily_spent_units = None;
        assert_eq!(
            evaluate(&policy, profile, &ctx).code,
            "DAILY_ACCOUNTING_REQUIRED"
        );
        ctx.daily_spent_units = Some(5_000_000);
        ctx.now = 86_399;
    }
}
#[test]
fn legacy_source_and_substituted_day_accounting_are_rejected() {
    assert!(compile("pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap(ctx, \"10\", \"USDC\")?; Ok(()) }").is_err());
    for args in [
        "None, ctx.spent_day, ctx.now, ctx.amount_units",
        "ctx.daily_spent_units, None, ctx.now, ctx.amount_units",
        "ctx.daily_spent_units, ctx.spent_day, 0, ctx.amount_units",
        "ctx.daily_spent_units, ctx.spent_day, ctx.now, 0",
    ] {
        let source = format!(
            "use allowit::v1::prelude::*; struct PolicyParams {{}} fn new()->PolicyParams{{PolicyParams{{}}}} async fn _execute(ctx:&Context,params:&PolicyParams)->PolicyResult{{allowit::set_cap_per_day({args},&ctx.token,10_000_000,\"USDC\",6)?;Ok(())}}"
        );
        assert!(compile(&source).is_err());
    }
}
#[test]
fn direct_primitive_matches_daily_guard_and_preserves_overflow_denial() {
    assert!(
        allowit_sdk::set_cap_per_day(Some(u64::MAX), Some(0), 0, 1, "USDC", u64::MAX, "USDC", 6)
            .is_err()
    );
    assert!(
        allowit_sdk::set_cap_per_day(Some(u64::MAX), Some(0), 86400, 1, "USDC", 1, "USDC", 6)
            .is_ok()
    );
    assert!(allowit_sdk::set_cap_per_day(None, Some(0), 0, 1, "USDC", 1, "USDC", 6).is_err());
}

#[test]
fn declared_tempo_asset_daily_and_action_guards_use_verified_current_day_state() {
    let source = r#"use allowit::v1::prelude::*;
struct PolicyParams {} fn new() -> PolicyParams { PolicyParams {} }
async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::require_asset(&ctx.token, "AlphaUSD")?;
    allowit::set_cap_per_day(ctx.daily_spent_units, ctx.spent_day, ctx.now, ctx.amount_units, &ctx.token, 1_000_000, "AlphaUSD", 6)?;
    allowit::cap_per_transaction(ctx.amount_units, &ctx.token, 500_000, "AlphaUSD", 6)?;
    allowit::require_recipient(&ctx.recipient, "supplier")?;
    Ok(())
}"#;
    let policy = compile(source).unwrap();
    assert_eq!(policy.token, "AlphaUSD");
    assert_eq!(policy.daily_limit, "1");
    let mut ctx = context();
    ctx.network = "tempo:localnet".into();
    ctx.token = "AlphaUSD".into();
    ctx.recipient = "supplier".into();
    ctx.now = 86_399;
    ctx.spent_day = Some(0);
    ctx.daily_spent_units = Some(750_000);
    ctx.amount_units = 250_000;
    ctx.native_policy_storage = Some(allowit_sdk::NativePolicyStorage {
        daily_limit_units: 1_000_000,
        action_limit_units: 500_000,
    });
    let asset = allowit_sdk::ServicePaymentAsset {
        network: "tempo:localnet".into(),
        asset: "AlphaUSD".into(),
        decimals: 6,
    };
    let run =
        |ctx: &Context| allowit_sdk::evaluate_with_trace_native_payment(&policy, ctx, &asset).0;
    assert_eq!(run(&ctx).outcome, "pass");
    ctx.daily_spent_units = Some(750_001);
    assert_eq!(run(&ctx).code, "DAILY_CAP_EXCEEDED");
    ctx.now = 86_400;
    assert_eq!(run(&ctx).outcome, "pass");
    ctx.amount_units = 500_001;
    assert_ne!(run(&ctx).outcome, "pass");
    ctx.amount_units = 250_000;
    ctx.recipient = "different".into();
    assert_ne!(run(&ctx).outcome, "pass");
    // A source declaration does not authenticate a public caller's token data.
    assert_eq!(
        evaluate(&policy, Profile::Oracle, &ctx).code,
        "TOKEN_MISMATCH"
    );
}

#[test]
fn non_usdc_caps_require_one_matching_unconditional_asset_and_six_decimals() {
    let source = r#"use allowit::v1::prelude::*; struct PolicyParams {} fn new()->PolicyParams{PolicyParams{}}
async fn _execute(ctx:&Context,params:&PolicyParams)->PolicyResult {
    allowit::require_asset(&ctx.token,"AlphaUSD")?;
    allowit::set_cap_per_day(ctx.daily_spent_units,ctx.spent_day,ctx.now,ctx.amount_units,&ctx.token,1_000_000,"AlphaUSD",6)?;
    allowit::cap_per_transaction(ctx.amount_units,&ctx.token,500_000,"AlphaUSD",6)?;
    Ok(())
}"#;
    assert!(compile(source).is_ok());
    for invalid in [
        source.replace("allowit::require_asset(&ctx.token,\"AlphaUSD\")?;", ""),
        source.replace(
            "require_asset(&ctx.token,\"AlphaUSD\")",
            "require_asset(&ctx.token,\"BetaUSD\")",
        ),
        source.replace("500_000,\"AlphaUSD\",6", "500_000,\"BetaUSD\",6"),
        source.replace("1_000_000,\"AlphaUSD\",6", "1_000_000,\"AlphaUSD\",18"),
        source.replace(
            "allowit::require_asset(&ctx.token,\"AlphaUSD\")?;",
            "if ctx.amount_units > 0 { allowit::require_asset(&ctx.token,\"AlphaUSD\")?; }",
        ),
    ] {
        assert!(compile(&invalid).is_err(), "{invalid}");
    }
}

#[test]
fn daily_accounting_errors_preserve_primitive_facade_and_runtime_diagnostics() {
    let policy = compile("pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap_per_day(ctx, \"10\", \"USDC\")?; Ok(()) }").unwrap();
    for (spent, day, now) in [
        (None, Some(0), 0),
        (Some(0), None, 0),
        (Some(0), Some(1), 0),
    ] {
        let mut ctx = context();
        ctx.daily_spent_units = spent;
        ctx.spent_day = day;
        ctx.now = now;
        let primitive = allowit_sdk::set_cap_per_day(
            spent,
            day,
            now,
            ctx.amount_units,
            "USDC",
            10_000_000,
            "USDC",
            6,
        )
        .unwrap_err();
        let facade = allowit_sdk::prelude::set_cap_per_day(&ctx, "10", "USDC").unwrap_err();
        assert_eq!(facade, primitive);
        for profile in [Profile::Contract, Profile::Oracle] {
            let decision = evaluate(&policy, profile, &ctx);
            assert_eq!(decision.code, primitive.code);
            assert_eq!(decision.reason, primitive.reason);
        }
    }
}
