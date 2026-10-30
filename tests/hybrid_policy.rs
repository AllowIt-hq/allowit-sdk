use allowit::prelude::*;
use allowit::{
    PolicyRuleFacts, PolicyScalar, RuleExpense, TrustedPolicyState, compile,
    evaluate_with_policy_rules,
};
use allowit_sdk as allowit;

// This function is also checked by rustc, including the one-argument facade and normal error.
#[allow(dead_code)]
async fn authored_rust(ctx: &Context) -> PolicyResult {
    allowit::spent_cap(grant::spent_cap)?;
    allowit::recipients(grant::recipients)?;
    if ctx.amount_units > 100_000_000 {
        return Err(PolicyError {
            code: "AMOUNT_LIMIT".into(),
            reason: "Amount exceeds the custom limit.".into(),
        });
    }
    Ok(())
}
fn source(body: &str) -> String {
    format!("use allowit::prelude::*; async fn _execute(ctx:&Context)->PolicyResult {{ {body} }}")
}
fn context() -> Context {
    let mut ctx = Context::default();
    ctx.network = "stellar:testnet".into();
    ctx.token = "XLM".into();
    ctx.action = "stellar::sep56_deposit".into();
    ctx.amount_units = 100;
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
fn run(body: &str, ctx: &Context) -> allowit::Decision {
    evaluate_with_policy_rules(&compile(&source(body)).unwrap(), ctx)
}
#[test]
fn prefix_preserves_order_and_stops_before_custom_code() {
    let compiled=compile(&source("allowit::recipients(grant::recipients)?; allowit::spent_cap(grant::spent_cap)?; allowit::spent_cap(grant::spent_cap)?; if ctx.amount_units>0 {allowit::tokens(grant::tokens)?;} allowit::expiry(grant::expiry)?; Ok(())")).unwrap();
    assert_eq!(
        compiled.native_prefix,
        ["recipients", "spent_cap", "spent_cap"]
    );
    assert!(compiled.offchain_required);
    let compiled = compile(&source(
        "allowit::transaction_cap(200)?; allowit::recipients(grant::recipients)?; Ok(())",
    ))
    .unwrap();
    assert!(compiled.native_prefix.is_empty());
}
#[test]
fn literal_and_computed_rules_execute_offchain() {
    let ctx = context();
    assert_eq!(run("let limit=ctx.amount_units+1; allowit::transaction_cap(limit)?; allowit::recipients(&[\"VAULT\"])?; Ok(())",&ctx).outcome,"pass");
    assert_eq!(
        run("allowit::transaction_cap(99)?; Ok(())", &ctx).code,
        "TRANSACTION_CAP_EXCEEDED"
    );
}
#[test]
fn null_and_unreached_rules_do_not_request_expense_facts() {
    let mut ctx = context();
    ctx.rule_facts.as_mut().unwrap().expense = None;
    assert_eq!(run("allowit::spent_cap(grant::spent_cap)?; if false {allowit::day_total_cap(100)?;} Ok(())",&ctx).outcome,"pass");
    ctx.policy_state
        .as_mut()
        .unwrap()
        .grant
        .insert("spent_cap".into(), PolicyScalar::U64(10));
    assert_eq!(
        run("allowit::spent_cap(grant::spent_cap)?; Ok(())", &ctx).code,
        "EXPENSE_BINDING_REQUIRED"
    );
    assert_eq!(
        run("allowit::spent_cap(None)?; Ok(())", &ctx).outcome,
        "pass"
    );
}
#[test]
fn native_route_has_no_implicit_null_grant() {
    let mut ctx = context();
    ctx.rule_facts.as_mut().unwrap().grant_available = false;
    assert_eq!(
        run("allowit::spent_cap(grant::spent_cap)?; Ok(())", &ctx).code,
        "NATIVE_GRANT_UNAVAILABLE"
    );
    assert_eq!(run("allowit::spent_cap(10)?; Ok(())", &ctx).outcome, "pass");
}
#[test]
fn expenses_and_gross_principal_are_distinct_and_idempotent() {
    let ctx = context();
    let body =
        "allowit::spent_cap(3)?; allowit::day_spent_cap(3)?; allowit::day_total_cap(101)?; Ok(())";
    assert_eq!(run(body, &ctx).outcome, "pass");
    assert_eq!(run(body, &ctx).outcome, "pass");
    let props = ctx.policy_state.as_ref().unwrap().properties.borrow();
    assert_eq!(props.get("spent"), Some(&PolicyScalar::U64(3)));
    assert_eq!(props.get("day_total"), Some(&PolicyScalar::U64(101)));
}
#[test]
fn ordinary_error_discards_all_working_state() {
    let ctx = context();
    let result = run(
        "allowit::call_cap(5)?; if ctx.amount_units>0 {return Err(PolicyError {code: \"DENIED\".into(),reason: \"Custom condition denied.\".into()});} Ok(())",
        &ctx,
    );
    assert_eq!(result.code, "DENIED");
    assert!(
        ctx.policy_state
            .as_ref()
            .unwrap()
            .properties
            .borrow()
            .is_empty()
    );
}
#[test]
fn expiry_is_exclusive_and_null_is_optional() {
    let mut ctx = context();
    ctx.rule_facts.as_mut().unwrap().now = 10;
    assert_eq!(
        run("allowit::expiry(10)?; Ok(())", &ctx).code,
        "POLICY_EXPIRED"
    );
    assert_eq!(run("allowit::expiry(11)?; Ok(())", &ctx).outcome, "pass");
    assert_eq!(
        run("allowit::expiry(0)?; Ok(())", &ctx).code,
        "POLICY_EXPIRED"
    );
    assert_eq!(
        run("allowit::expiry(grant::expiry)?; Ok(())", &ctx).outcome,
        "pass"
    );
}
#[test]
fn removed_spellings_reject_fresh_source() {
    for name in [
        "fail",
        "state_remove",
        "total_cap",
        "daily_cap",
        "recipient",
        "token",
        "cap_per_transaction",
        "set_cap_per_day",
        "amount_at_most",
        "require_asset",
        "require_recipient",
        "allow_actions",
    ] {
        assert!(
            compile(&source(&format!("allowit::{name}(100)?; Ok(())"))).is_err(),
            "{name}"
        );
    }
}

#[test]
fn original_template_denies_with_ordinary_error() {
    let policy = compile(include_str!("../templates/policy/src/policy.rs")).unwrap();
    assert_eq!(
        evaluate_with_policy_rules(&policy, &context()).code,
        "POLICY_NOT_CONFIGURED"
    );
}
