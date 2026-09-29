use allowit_sdk::{ConfidenceInterval, Context, Profile, compile, evaluate, semantic_evidence_key};
use serde_json::json;
fn source(body: &str) -> String {
    format!("pub async fn evaluate(ctx: &Context) -> PolicyResult {{ {body} }}")
}
fn context() -> Context {
    serde_json::from_str(include_str!("../examples/green-context.json")).unwrap()
}

#[test]
fn research_category_requires_assessment_regardless_of_caller_label() {
    let policy = compile(include_str!("../examples/research.rs")).unwrap();
    let mut ctx: Context = serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    let question =
        "Does this purchase count as research under the user's stated purpose and definitions?";
    for label in ["research", "study", "caller-chosen-label"] {
        ctx.action = label.into();
        ctx.confidence.clear();
        let missing = evaluate(&policy, Profile::Oracle, &ctx);
        assert_eq!(missing.code, "SEMANTIC_EVIDENCE_REQUIRED");
        assert_eq!(missing.question.as_deref(), Some(question));
        for (score, outcome) in [(8500, "pass"), (4000, "fail")] {
            ctx.confidence.insert(
                semantic_evidence_key(question),
                ConfidenceInterval {
                    lower_bps: score,
                    upper_bps: score,
                },
            );
            assert_eq!(evaluate(&policy, Profile::Oracle, &ctx).outcome, outcome);
        }
    }
}

#[test]
fn semantic_request_identifies_exact_question_and_requires_original_intent() {
    let question = "Does this request avoid hype?";
    let p=compile(&source(&format!("let score = semantic(ctx, \"{question}\")?; if score.lower_bps < 8000 {{ return fail(\"Too much hype\"); }} Ok(())"))).unwrap();
    let mut ctx = context();
    let missing = evaluate(&p, Profile::Oracle, &ctx);
    assert_eq!(missing.outcome, "fail");
    assert_eq!(missing.code, "SEMANTIC_EVIDENCE_REQUIRED");
    assert_eq!(missing.question.as_deref(), Some(question));
    assert_eq!(missing.evidence_key, Some(semantic_evidence_key(question)));
    assert_eq!(
        evaluate(&p, Profile::Contract, &ctx).code,
        "SEMANTIC_EVIDENCE_REQUIRED"
    );
    ctx.confidence.insert(
        semantic_evidence_key(question),
        ConfidenceInterval {
            lower_bps: 8000,
            upper_bps: 8000,
        },
    );
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).outcome, "pass");
    assert_eq!(evaluate(&p, Profile::Contract, &ctx).outcome, "pass");
    ctx.confidence
        .get_mut(&semantic_evidence_key(question))
        .unwrap()
        .lower_bps = 7999;
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "POLICY_REJECTED");
    ctx.original_intent.clear();
    assert_eq!(
        evaluate(&p, Profile::Oracle, &ctx).code,
        "ORIGINAL_INTENT_REQUIRED"
    );
}
#[test]
fn deterministic_return_gap_cannot_be_overridden_by_a_high_semantic_score() {
    let p = compile(include_str!("../examples/green-investments.rs")).unwrap();
    let mut ctx = context();
    for question in [
        "Does this transaction count as an investment under the user's stated purpose and definitions?",
        "Does this investment prioritize credible environmental benefits and avoid hype?",
    ] {
        let missing = evaluate(&p, Profile::Oracle, &ctx);
        assert_eq!(missing.code, "SEMANTIC_EVIDENCE_REQUIRED");
        assert_eq!(missing.question.as_deref(), Some(question));
        ctx.confidence.insert(
            missing.evidence_key.unwrap(),
            ConfidenceInterval {
                lower_bps: 10000,
                upper_bps: 10000,
            },
        );
    }
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).outcome, "pass");
    ctx.runtime_context["candidate_yield_bps"] = json!(400);
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).outcome, "pass");
    ctx.runtime_context["candidate_yield_bps"] = json!(399);
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "POLICY_REJECTED");
    ctx.runtime_context["candidate_yield_bps"] = json!(u64::MAX);
    // A higher candidate return passes the gap check without overflowing candidate + gap.
    // This numeric comparison does not authenticate the supplied return estimate.
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "PASS");
}
#[test]
fn context_values_are_strict_u64_not_coerced() {
    let p = compile(&source("let value = context_u64(ctx, \"yield\")?; Ok(())")).unwrap();
    let mut ctx = context();
    assert_eq!(
        evaluate(&p, Profile::Oracle, &ctx).code,
        "CONTEXT_VALUE_REQUIRED"
    );
    for value in [
        json!(-1),
        json!(1.5),
        json!("100"),
        json!(true),
        json!(null),
    ] {
        ctx.runtime_context["yield"] = value;
        assert_eq!(
            evaluate(&p, Profile::Oracle, &ctx).code,
            "INVALID_CONTEXT_VALUE"
        );
    }
    ctx.runtime_context["yield"] = json!(0);
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).outcome, "pass");
}
#[test]
fn runtime_json_and_original_intent_are_bounded() {
    let p = compile(&source("Ok(())")).unwrap();
    let mut ctx = context();
    ctx.original_intent = "x".repeat(16385);
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "INVALID_CONTEXT");
    ctx = context();
    ctx.runtime_context = json!(null);
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "INVALID_CONTEXT");
    ctx.runtime_context = json!({"many":vec![0;129]});
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "INVALID_CONTEXT");
    ctx.runtime_context = json!({"large":"x".repeat(16385)});
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "INVALID_CONTEXT");
    let mut deep = json!(0);
    for _ in 0..9 {
        deep = json!({"child":deep});
    }
    ctx.runtime_context = deep;
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "INVALID_CONTEXT");
}
