use allowit_sdk::{Context, Profile, compile, evaluate};

fn source(body: &str) -> String {
    format!(
        "use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult {{ {body} }}"
    )
}
fn context() -> Context {
    serde_json::from_str(include_str!("../examples/context.json")).unwrap()
}

#[test]
fn private_handler_preserves_outer_limits_and_all_check_results() {
    let policy = compile(&source(
        r#"allowit::set_cap(ctx, "5", "USDC")?;
allowit::cap_per_transaction(ctx, "2", "USDC")?;
allowit::require_recipient(ctx, "11111111111111111111111111111111")?;
Ok(())"#,
    ))
    .unwrap();
    for profile in [Profile::Oracle, Profile::Contract] {
        let mut ctx = context();
        ctx.amount_units = 2_000_000;
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "pass");
        ctx.amount_units = 2_000_001;
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "fail");
        ctx.amount_units = 2_000_000;
        ctx.spent_units = 3_000_001;
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "fail");
        ctx.spent_units = 0;
        ctx.recipient = "wrong-recipient".into();
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "fail");
        ctx.recipient = "11111111111111111111111111111111".into();
        ctx.allocation_units = 1_000_000;
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "fail");
    }
}

#[test]
fn private_handler_rejects_missing_bindings_even_in_unreachable_code() {
    for body in [
        "let amount = REPORT_PAYMENT; Ok(())",
        "if false { let amount = REPORT_PAYMENT; } Ok(())",
        "return Ok(()); let amount = REPORT_PAYMENT;",
        "if false { paysh::pay(ctx)?; } Ok(())",
        "allowit::set_cap(ctx, REPORT_BUDGET, \"USDC\")?; Ok(())",
        "allowit::set_cap(ctx, \"5\", \"USDC\"); Ok(())",
        "if false { allowit::set_cap(ctx, \"5\", \"USDC\")?; } Ok(())",
        "_execute(ctx).await?; Ok(())",
    ] {
        assert!(compile(&source(body)).is_err(), "accepted: {body}");
    }
}

#[test]
fn private_handler_has_one_exact_signature_and_no_ambient_items() {
    for signature in [
        "pub async fn _execute(ctx: &Context) -> PolicyResult",
        "pub(crate) async fn _execute(ctx: &Context) -> PolicyResult",
        "async fn execute(ctx: &Context) -> PolicyResult",
        "fn _execute(ctx: &Context) -> PolicyResult",
        "async fn _execute(payment: &SolanaTransfer) -> PolicyResult",
        "async fn _execute(ctx: &Context, budget: u64) -> PolicyResult",
        "async fn _execute(ctx: &Context) -> Result<()>",
    ] {
        assert!(
            compile(&format!("{signature} {{ Ok(()) }}")).is_err(),
            "{signature}"
        );
    }
    for extra in [
        "const REPORT_PAYMENT: u64 = 5;",
        "fn helper() {}",
        "static BUDGET: u64 = 5;",
    ] {
        assert!(compile(&format!("{extra} {}", source("Ok(())"))).is_err());
        assert!(compile(&format!("{} {extra}", source("Ok(())"))).is_err());
    }
    assert!(compile("async fn _execute(ctx: &Context) -> PolicyResult { Ok(()) }").is_ok());
}

#[test]
fn private_handler_keeps_hoisted_budget_before_early_returns() {
    let policy = compile(&source(
        r#"return Ok(()); allowit::set_cap(ctx, "1", "USDC")?;"#,
    ))
    .unwrap();
    for profile in [Profile::Oracle, Profile::Contract] {
        let mut ctx = context();
        ctx.amount_units = 1_000_001;
        assert_eq!(evaluate(&policy, profile, &ctx).outcome, "fail");
    }
}

#[allow(dead_code, unused_variables)]
mod generated_source_typecheck {
    use allowit_sdk as allowit;
    include!("fixtures/generated-small-payments.rs");

    #[test]
    fn exact_generated_source_typechecks_as_rust() {
        let _ = _execute;
    }
}

#[allow(dead_code, unused_variables)]
mod generated_invoice_typecheck {
    use allowit_sdk as allowit;
    include!("fixtures/generated-invoice.rs");
    #[test]
    fn exact_generated_source_typechecks_as_rust() {
        let _ = _execute;
    }
}

#[allow(dead_code, unused_variables)]
mod generated_owner_review_typecheck {
    use allowit_sdk as allowit;
    include!("fixtures/generated-owner-review.rs");
    #[test]
    fn exact_generated_source_typechecks_as_rust() {
        let _ = _execute;
    }
}
