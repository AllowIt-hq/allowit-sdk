use allowit_sdk::{ConfidenceInterval, Context, Profile, compile, evaluate, semantic_evidence_key};
fn source(approve: bool, deny: bool) -> String {
    format!(
        "pub async fn evaluate(ctx: &Context) -> PolicyResult {{ set_cap(ctx, \"500\", \"USDC\")?; check_preference(ctx, \"Avoid hype 🌱\", {approve}, \"85\", {deny}, \"40\").await?; Ok(()) }}"
    )
}
fn context(score: u64) -> Context {
    let mut c: Context =
        serde_json::from_str(include_str!("../examples/green-context.json")).unwrap();
    c.confidence.insert(
        semantic_evidence_key("Avoid hype 🌱"),
        ConfidenceInterval {
            lower_bps: score,
            upper_bps: score,
        },
    );
    c
}
#[test]
fn thresholds_flags_and_contract_answers_are_enforced() {
    for (approve, deny, score, outcome) in [
        (true, true, 8500, "pass"),
        (true, true, 8499, "awaiting_input"),
        (true, true, 4000, "fail"),
        (true, true, 4001, "awaiting_input"),
        (false, true, 10000, "awaiting_input"),
        (true, false, 0, "awaiting_input"),
        (false, false, 5000, "awaiting_input"),
    ] {
        let p = compile(&source(approve, deny)).unwrap();
        let mut c = context(score);
        let d = evaluate(&p, Profile::Oracle, &c);
        assert_eq!(d.outcome, outcome, "{approve} {deny} {score}");
        if let Some(key) = d.input_key {
            c.answers.insert(key, true);
            assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, "pass");
            assert_eq!(
                evaluate(&p, Profile::Contract, &c).code,
                "USER_INPUT_REQUIRED"
            );
        }
    }
}
#[test]
fn workflow_and_call_spans_are_exact_and_nested_steps_stay_conditional() {
    let s = source(true, true);
    let p = compile(&s).unwrap();
    assert_eq!(
        p.workflow.iter().filter(|w| w.kind == "preference").count(),
        1
    );
    for w in &p.workflow {
        assert_eq!(
            String::from_utf16(&s.encode_utf16().collect::<Vec<_>>()[w.start..w.end]).unwrap(),
            w.source
        );
    }
    for c in &p.calls {
        assert_eq!(
            String::from_utf16(&s.encode_utf16().collect::<Vec<_>>()[c.start..c.end]).unwrap(),
            c.name
        );
    }
    let nested = s
        .replace(
            "check_preference",
            "if ctx.amount_units > 1 { check_preference",
        )
        .replace(".await?;", ".await?; }");
    let p = compile(&nested).unwrap();
    assert!(!p.workflow.iter().any(|w| w.kind == "preference"));
    assert!(
        p.workflow
            .iter()
            .any(|w| w.kind == "custom" && w.source.contains("check_preference"))
    );
    let s = "pub async fn evaluate(ctx: &Context) -> PolicyResult { let fit = semantic(ctx, \"Avoid hype\")?; if fit.lower_bps < 8500 { return fail(\"No\"); } Ok(()) }";
    assert_eq!(compile(s).unwrap().workflow[0].name, "semantic");
}
#[test]
fn invalid_settings_cannot_compile_or_bypass_hard_rules() {
    let s = source(true, true);
    for bad in [
        s.replace("\"40\"", "\"85\""),
        s.replace("\"40\"", "\"101\""),
        s.replace(".await?;", "?;"),
        s.replace(".await?;", ".await;"),
        s.replace("Ok(())", "let __allowit_x = 1; Ok(())"),
    ] {
        assert!(compile(&bad).is_err(), "{bad}");
    }
    let p = compile(&s).unwrap();
    let mut c = context(10000);
    c.amount_units = 501_000_000;
    c.allocation_units = 1_000_000_000;
    assert_eq!(
        evaluate(&p, Profile::Oracle, &c).code,
        "POLICY_CAP_EXCEEDED"
    );
    let mut c = context(10000);
    c.network = "local:dev".into();
    assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, "pass");
    assert_eq!(evaluate(&p, Profile::Contract, &c).code, "INVALID_NETWORK");
    c.confidence.clear();
    assert_eq!(
        evaluate(&p, Profile::Oracle, &c).code,
        "SEMANTIC_EVIDENCE_REQUIRED"
    );
}

#[test]
fn manual_only_does_not_require_model_evidence_and_reserved_reads_fail() {
    let p = compile(&source(false, false)).unwrap();
    let mut c = context(0);
    c.confidence.clear();
    assert_eq!(evaluate(&p, Profile::Oracle, &c).outcome, "awaiting_input");
    assert_eq!(
        evaluate(&p, Profile::Contract, &c).code,
        "USER_INPUT_REQUIRED"
    );
    for body in [
        "let x = __allowit_preference_0;",
        "let x = __allowit_preference_0.lower_bps;",
    ] {
        assert!(compile(&source(true, true).replace("Ok(())", &format!("{body} Ok(())"))).is_err());
    }
    for n in [1025, 2000] {
        assert!(compile(&source(true, true).replace("Avoid hype 🌱", &"x".repeat(n))).is_err());
    }
}

#[test]
fn suffixed_strings_are_not_valid_rust_policy_literals() {
    for s in [
        source(true, true).replace("\"Avoid hype 🌱\"", "\"Avoid hype 🌱\"x"),
        source(true, true).replace("\"85\"", "\"85\"percent"),
    ] {
        assert!(compile(&s).is_err());
    }
}
