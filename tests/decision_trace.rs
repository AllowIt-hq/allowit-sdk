use allowit_sdk::{Context, Profile, compile, evaluate};

fn context() -> Context {
    Context {
        amount_units: 750_000,
        allocation_units: 10_000_000,
        token: "USDC".into(),
        network: "local:dev".into(),
        action: "research".into(),
        original_intent: "Research primary sources".into(),
        daily_spent_units: Some(0),
        spent_day: Some(0),
        ..Context::default()
    }
}
fn policy(body: &str) -> String {
    format!("pub async fn exec(ctx: &Context) -> PolicyResult {{  {body} Ok(()) }}")
}
#[test]
fn reports_actual_call_not_first_similar_rule() {
    let source =
        policy("allowit::transaction_cap(1_000_000)?; allowit::transaction_cap(500_000)?;");
    let compiled = compile(&source).unwrap();
    let d = allowit_sdk::evaluate_with_policy_rules(&compiled, &rule_context());
    assert_eq!(d.code, "TRANSACTION_CAP_EXCEEDED");
    assert_eq!(d.source_start, source.rfind("allowit::transaction_cap"));
    assert_eq!(
        &source[d.source_start.unwrap()..d.source_end.unwrap()],
        "allowit::transaction_cap"
    );
}
#[test]
fn maps_lowered_preference_and_custom_input_to_exact_source() {
    for body in [
        "allowit::v1::prelude::check_preference(ctx, \"Primary 🌱 evidence?\", None, None).await?;",
        "if ctx.amount_units > 500000 { allowit::require_user_input(ctx, \"Review 🌱\").await?; }",
    ] {
        let source = policy(body);
        let compiled = compile(&source).unwrap();
        let d = evaluate(&compiled, Profile::Oracle, &context());
        assert_eq!(d.outcome, "awaiting_input");
        let slice = &source[d.source_start.unwrap()..d.source_end.unwrap()];
        assert!(
            slice.contains("check_preference") || slice.contains("require_user_input"),
            "{slice}"
        );
        assert!(
            source[d.source_start.unwrap()..].starts_with(body)
                || source[d.source_start.unwrap()..].starts_with("allowit::require_user_input")
        );
    }
}
#[test]
fn arithmetic_failure_gets_containing_statement_but_host_checks_have_no_span() {
    let source = policy("let broken = 1 / 0;");
    let compiled = compile(&source).unwrap();
    let d = evaluate(&compiled, Profile::Oracle, &context());
    assert_eq!(d.code, "ARITHMETIC_ERROR");
    assert_eq!(
        &source[d.source_start.unwrap()..d.source_end.unwrap()],
        "let broken = 1 / 0;"
    );
    let mut invalid = context();
    invalid.allocation_units = 1;
    let d = evaluate(&compiled, Profile::Oracle, &invalid);
    assert_eq!(d.code, "BUDGET_EXCEEDED");
    assert!(d.source_start.is_none() && d.source_end.is_none());
}

fn rule_context() -> Context {
    let mut c = context();
    c.policy_state = Some(allowit_sdk::TrustedPolicyState::default());
    c.rule_facts = Some(allowit_sdk::PolicyRuleFacts {
        operation_id: "decision-trace".into(),
        grant_available: true,
        amount_units: c.amount_units,
        asset_id: "SAC".into(),
        recipient: "recipient".into(),
        now: 0,
        expense: None,
    });
    c
}
