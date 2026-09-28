use allowit_sdk::{ConfidenceInterval, Context, Profile, compile, evaluate, semantic_evidence_key};
fn context() -> Context {
    Context {
        amount_units: 750_000,
        allocation_units: 10_000_000,
        token: "USDC".into(),
        network: "local:dev".into(),
        action: "research".into(),
        original_intent: "Research primary sources".into(),
        purchase_counts: Some(vec![0; 40]),
        ..Context::default()
    }
}
fn source(body: &str) -> String {
    format!(
        "use allowit::v1::prelude::*; pub async fn exec(ctx: &Context) -> PolicyResult {{ set_cap(ctx, \"10\", \"USDC\")?; {body} Ok(()) }}"
    )
}
#[test]
fn compact_preference_and_versioned_exec_preserve_outcomes() {
    for (deny, approve) in [("0.40", "0.85"), ("auto(\"deny\")", "auto(\"approve\")")] {
        let p = compile(&source(&format!(
            "check_preference(ctx, \"Primary evidence?\", {deny}, {approve}).await?;"
        )))
        .unwrap();
        assert_eq!(
            p.workflow
                .iter()
                .find(|n| n.kind == "preference")
                .unwrap()
                .arguments,
            ["Primary evidence?", "true", "85.00", "true", "40.00"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        for (score, outcome) in [
            (4000, "fail"),
            (4001, "awaiting_input"),
            (8499, "awaiting_input"),
            (8500, "pass"),
        ] {
            let mut c = context();
            c.confidence.insert(
                semantic_evidence_key("Primary evidence?"),
                ConfidenceInterval {
                    lower_bps: score,
                    upper_bps: score,
                },
            );
            assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, outcome);
        }
    }
    let p = compile(&source(
        "check_preference(ctx, \"Ask\", None, None).await?;",
    ))
    .unwrap();
    assert_eq!(
        evaluate(&p, Profile::Oracle, &context()).outcome,
        "awaiting_input"
    );
    for bad in [
        "1.01",
        "0.12345",
        "-0.1",
        "0.5f32",
        "1e-1",
        "0_1",
        "auto(\"approve\")",
    ] {
        assert!(
            compile(&source(&format!(
                "check_preference(ctx, \"Ask\", {bad}, 0.85).await?;"
            )))
            .is_err(),
            "{bad}"
        );
    }
    for header in ["allowit::v2::prelude", "other::v1::prelude"] {
        assert!(compile(&source("").replace("allowit::v1::prelude", header)).is_err());
    }
}
#[test]
fn price_bands_include_boundaries_without_a_minimum_and_need_trusted_counts() {
    let body = "cap_purchase_tiers(ctx, \"1\", 2, \"USDC\")?;";
    let p = compile(&source(body)).unwrap();
    let mut c = context();
    c.purchase_counts.as_mut().unwrap()[0] = 2;
    assert_eq!(
        evaluate(&p, Profile::Oracle, &c).code,
        "PURCHASE_TIER_EXCEEDED"
    );
    c.amount_units = 500_000;
    assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, "pass");
    c.purchase_counts.as_mut().unwrap()[1] = 4;
    assert_eq!(
        evaluate(&p, Profile::Oracle, &c).code,
        "PURCHASE_TIER_EXCEEDED"
    );
    c.amount_units = 250_000;
    assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, "pass");
    c.amount_units = 1;
    assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, "pass");
    c.amount_units = 1_000_001;
    assert_eq!(
        evaluate(&p, Profile::Oracle, &c).code,
        "PURCHASE_CAP_EXCEEDED"
    );
    c.amount_units = 10;
    c.purchase_counts = None;
    assert_eq!(evaluate(&p, Profile::Oracle, &c).code, "LEDGER_REQUIRED");
    c.network = "solana:devnet".into();
    c.purchase_counts = Some(vec![0; 40]);
    assert_eq!(evaluate(&p, Profile::Contract, &c).code, "LEDGER_REQUIRED");
    for bad in [
        format!("{body} {body}"),
        format!("if ctx.amount_units > 0 {{ {body} }}"),
        body.replace(", 2,", ", 0,"),
        body.replace("\"1\"", "\"1000001\""),
    ] {
        assert!(compile(&source(&bad)).is_err());
    }
}

#[test]
fn early_returns_and_earlier_questions_cannot_bypass_purchase_tiers() {
    for before in [
        "return Ok(());",
        "if ctx.amount_units > 0 { return Ok(()); }",
        "require_user_input(ctx, \"Ask first\").await?;",
    ] {
        let p = compile(&source(&format!(
            "{before} cap_purchase_tiers(ctx, \"1\", 2, \"USDC\")?;"
        )))
        .unwrap();
        let mut c = context();
        c.purchase_counts.as_mut().unwrap()[0] = 2;
        assert_eq!(
            evaluate(&p, Profile::Oracle, &c).code,
            "PURCHASE_TIER_EXCEEDED"
        );
        c.network = "solana:devnet".into();
        assert_eq!(evaluate(&p, Profile::Contract, &c).code, "LEDGER_REQUIRED");
    }
}

extern crate allowit_sdk as allowit;
#[allow(dead_code)]
mod typecheck {
    use allowit::v1::prelude::*;
    pub async fn exec(ctx: &Context) -> PolicyResult {
        cap_purchase_tiers(ctx, "1", 2, "USDC")?;
        check_preference(ctx, "Ask", 0.40, 0.85).await?;
        check_preference(ctx, "Ask", None, auto("approve")).await?;
        check_preference(ctx, "Ask", 0, 1).await?;
        Ok(())
    }
}
