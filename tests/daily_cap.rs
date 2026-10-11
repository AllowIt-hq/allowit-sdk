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
