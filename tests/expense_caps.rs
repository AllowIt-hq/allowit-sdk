use allowit_sdk::{
    Context, PolicyRuleFacts, PolicyScalar, RuleExpense, TrustedPolicyState, compile,
    evaluate_with_policy_rules,
};
fn context() -> Context {
    let mut ctx = Context::default();
    ctx.network = "stellar:testnet".into();
    ctx.token = "XLM".into();
    ctx.action = "stellar::sep56_deposit".into();
    ctx.policy_state = Some(TrustedPolicyState::default());
    ctx.rule_facts = Some(PolicyRuleFacts {
        operation_id: "op1".into(),
        grant_available: true,
        amount_units: 100,
        asset_id: "SAC".into(),
        recipient: "VAULT".into(),
        now: 86401,
        expense: Some(RuleExpense {
            spent_units: 3,
            gas_units: 1,
        }),
    });
    ctx
}
fn run(body: &str, ctx: &Context) -> allowit_sdk::Decision {
    let source = format!(
        "use allowit::prelude::*;async fn _execute(ctx:&Context)->PolicyResult{{{body} Ok(())}}"
    );
    evaluate_with_policy_rules(&compile(&source).unwrap(), ctx)
}
#[test]
fn day_total_includes_principal_and_separate_gas() {
    let ctx = context();
    assert_eq!(
        run("allowit::day_total_cap(100)?;", &ctx).code,
        "POLICY_CAP_EXCEEDED"
    );
    assert_eq!(run("allowit::day_total_cap(101)?;", &ctx).outcome, "pass");
}
#[test]
fn expenses_exclude_retained_principal() {
    let ctx = context();
    assert_eq!(
        run("allowit::spent_cap(3)?; allowit::day_spent_cap(3)?;", &ctx).outcome,
        "pass"
    );
    assert_eq!(
        ctx.policy_state.unwrap().properties.borrow().get("spent"),
        Some(&PolicyScalar::U64(3))
    );
}
#[test]
fn daily_expenses_reset_at_utc_rollover_but_cumulative_expenses_do_not() {
    let mut ctx = context();
    assert_eq!(
        run("allowit::day_spent_cap(3)?; allowit::spent_cap(6)?;", &ctx).outcome,
        "pass"
    );
    let facts = ctx.rule_facts.as_mut().unwrap();
    facts.now += 86400;
    facts.operation_id = "op2".into();
    assert_eq!(
        run("allowit::day_spent_cap(3)?; allowit::spent_cap(6)?;", &ctx).outcome,
        "pass"
    );
    let values = ctx.policy_state.as_ref().unwrap().properties.borrow();
    assert_eq!(values.get("day_spent"), Some(&PolicyScalar::U64(3)));
    assert_eq!(values.get("spent"), Some(&PolicyScalar::U64(6)));
}
#[test]
fn protected_unknown_cost_history_fails_only_reached_nonnull_expense_rules() {
    let mut ctx = context();
    ctx.policy_state
        .as_mut()
        .unwrap()
        .properties
        .get_mut()
        .insert("unpriced".into(), PolicyScalar::Bool(true));
    assert_eq!(
        run("allowit::spent_cap(grant::spent_cap)?;", &ctx).outcome,
        "pass"
    );
    assert_eq!(
        run("if false {allowit::spent_cap(100)?;}", &ctx).outcome,
        "pass"
    );
    assert_eq!(
        run("allowit::spent_cap(100)?;", &ctx).code,
        "EXPENSE_HISTORY_UNPRICED"
    );
    assert_eq!(run("allowit::transaction_cap(100)?;", &ctx).outcome, "pass");
}
#[test]
fn unknown_daily_history_clears_only_after_day_rollover() {
    let mut ctx = context();
    let props = ctx.policy_state.as_mut().unwrap().properties.get_mut();
    props.insert("day".into(), PolicyScalar::U64(1));
    props.insert("day_unpriced".into(), PolicyScalar::Bool(true));
    assert_eq!(
        run("allowit::day_total_cap(200)?;", &ctx).code,
        "EXPENSE_HISTORY_UNPRICED"
    );
    ctx.rule_facts.as_mut().unwrap().now = 2 * 86400;
    assert_eq!(run("allowit::day_total_cap(101)?;", &ctx).outcome, "pass");
}
#[test]
fn overflow_and_clock_rollback_discard_working_state() {
    let mut ctx = context();
    let props = ctx.policy_state.as_mut().unwrap().properties.get_mut();
    props.insert("spent".into(), PolicyScalar::U64(u64::MAX));
    props.insert("day_spent_day".into(), PolicyScalar::U64(2));
    assert_eq!(
        run("allowit::spent_cap(18446744073709551615)?;", &ctx).code,
        "ARITHMETIC_ERROR"
    );
    assert_eq!(
        run("allowit::day_spent_cap(100)?;", &ctx).code,
        "INVALID_POLICY_STATE"
    );
}
#[test]
fn repeated_operation_cannot_change_accounting_facts() {
    let mut ctx = context();
    assert_eq!(run("allowit::spent_cap(100)?;", &ctx).outcome, "pass");
    ctx.rule_facts
        .as_mut()
        .unwrap()
        .expense
        .as_mut()
        .unwrap()
        .spent_units = 4;
    assert_eq!(
        run("allowit::spent_cap(100)?;", &ctx).code,
        "INVALID_POLICY_CONTEXT"
    );
}
