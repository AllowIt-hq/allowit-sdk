use allowit_sdk::{Context, Profile, compile, evaluate, evaluate_ir, prelude};
fn source(body: &str) -> String {
    format!("pub async fn evaluate(ctx: &Context) -> PolicyResult {{ {body} }}")
}
fn context() -> Context {
    serde_json::from_str(include_str!("../examples/context.json")).unwrap()
}

#[test]
fn exact_decimal_literals_reject_rounding_and_overflow() {
    for (value, expected) in [
        ("0", 0),
        ("25.50", 25_500_000),
        ("0.000001", 1),
        ("18446744073709.551615", u64::MAX),
    ] {
        assert_eq!(prelude::usdc(value).unwrap(), expected);
        let p = compile(&source(&format!("let amount = usdc(\"{value}\")?; if amount != {expected} {{ return fail(\"wrong amount\"); }} Ok(())"))).unwrap();
        assert_eq!(evaluate(&p, Profile::Oracle, &context()).outcome, "pass");
        assert!(!serde_json::to_string(&p.ir).unwrap().contains("usdc"));
    }
    for value in [
        "",
        ".1",
        "1.",
        "-1",
        "+1",
        "1e3",
        "1,000",
        "0.0000001",
        "1.0000000",
        "18446744073709.551616",
        "１",
        "1..1",
    ] {
        assert!(prelude::usdc(value).is_err(), "{value}");
        assert!(
            compile(&source(&format!("let amount = usdc(\"{value}\")?; Ok(())"))).is_err(),
            "{value}"
        );
    }
    for (value, expected) in [("0", 0), ("1", 100), ("85.25", 8525), ("100", 10000)] {
        assert_eq!(prelude::percent(value).unwrap(), expected);
        assert!(
            compile(&source(&format!(
                "let score = percent(\"{value}\")?; Ok(())"
            )))
            .is_ok()
        );
    }
    for value in ["100.01", "0.001", "-1", "1e2"] {
        assert!(prelude::percent(value).is_err());
        assert!(
            compile(&source(&format!(
                "let score = percent(\"{value}\")?; Ok(())"
            )))
            .is_err()
        );
    }
}

#[test]
fn inclusive_amount_checks_match_every_existing_execution_profile() {
    let p = compile(&source(
        "if !amount_at_most(ctx, \"10.000001\")? { return fail(\"Above purchase limit\"); } Ok(())",
    ))
    .unwrap();
    for (amount, expected) in [
        (10_000_000, "pass"),
        (10_000_001, "pass"),
        (10_000_002, "fail"),
    ] {
        let mut ctx = context();
        ctx.amount_units = amount;
        assert_eq!(
            prelude::amount_at_most(&ctx, "10.000001").unwrap(),
            expected == "pass"
        );
        for profile in [Profile::Oracle, Profile::Contract] {
            assert_eq!(evaluate(&p, profile, &ctx).outcome, expected);
            assert_eq!(evaluate_ir(&p.ir, profile, &ctx).outcome, expected);
        }
    }
    let mut ctx = context();
    ctx.token = "SOL".into();
    assert!(prelude::amount_at_most(&ctx, "10").is_err());
    assert_eq!(evaluate(&p, Profile::Contract, &ctx).outcome, "fail");
}

#[test]
fn return_gap_is_absolute_inclusive_and_safe_at_integer_bounds() {
    for (candidate, benchmark, expected) in [
        (400, 500, true),
        (399, 500, false),
        (600, 500, true),
        (u64::MAX, 0, true),
        (0, u64::MAX, false),
        (u64::MAX - 100, u64::MAX, true),
    ] {
        assert_eq!(
            prelude::within_percentage_points(candidate, benchmark, "1").unwrap(),
            expected
        );
        let p = compile(&source(&format!("let candidate = {candidate}; let benchmark = {benchmark}; if !within_percentage_points(candidate, benchmark, \"1\")? {{ return fail(\"Return gap exceeded\"); }} Ok(())"))).unwrap();
        assert_eq!(
            evaluate(&p, Profile::Contract, &context()).outcome == "pass",
            expected
        );
        let encoded = serde_json::to_string(&p.ir).unwrap();
        assert!(!encoded.contains("within_percentage_points"));
    }
}

#[test]
fn helpers_keep_exact_source_and_hover_spans_without_new_contract_opcodes() {
    let src = source(
        "// 🦀\nlet threshold = usdc(\"25.50\")?; if ctx.amount_units > threshold { return fail(\"Too much\"); } Ok(())",
    );
    let p = compile(&src).unwrap();
    let call = p.calls.iter().find(|c| c.name == "usdc").unwrap();
    let utf16: Vec<u16> = src.encode_utf16().collect();
    assert_eq!(
        String::from_utf16(&utf16[call.start..call.end]).unwrap(),
        "usdc"
    );
    let custom = p.workflow.iter().find(|n| n.kind == "custom").unwrap();
    assert!(custom.source.contains("usdc(\"25.50\")?"));
    assert_eq!(p.source, src);
    for body in [
        "let amount = usdc(\"1\"); Ok(())",
        "let value=\"1\";let amount=usdc(value)?;Ok(())",
        "let n = percent(101)?; Ok(())",
        "if within_percentage_points(percent(\"1\")?, 1, \"1\")? {} Ok(())",
        "if amount_at_most(1,\"10\")? {} Ok(())",
        "if within_percentage_points(true,false,\"1\")? {} Ok(())",
    ] {
        assert!(compile(&source(body)).is_err(), "{body}");
    }
}
