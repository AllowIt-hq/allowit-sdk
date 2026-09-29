#![cfg(feature = "compiler")]
use allowit_sdk::{
    Context, Profile, WorkflowStepStatus as S, WorkflowTrace, compile, evaluate,
    evaluate_with_trace, process_value, semantic_evidence_key,
};
use serde_json::json;

fn source(body: &str) -> String {
    format!(
        "use allowit::v1::prelude::*; pub async fn exec(ctx: &Context) -> PolicyResult {{ {body} }}"
    )
}
fn context() -> Context {
    Context {
        amount_units: 750_000,
        allocation_units: 10_000_000,
        action: "research".into(),
        token: "USDC".into(),
        network: "local:dev".into(),
        original_intent: "Buy primary evidence".into(),
        ..Context::default()
    }
}
fn traced(
    body: &str,
    ctx: &Context,
) -> (
    allowit_sdk::CompiledPolicy,
    allowit_sdk::Decision,
    WorkflowTrace,
) {
    let p = compile(&source(body)).unwrap();
    let (decision, trace) = evaluate_with_trace(&p, ctx);
    assert_eq!(
        decision,
        evaluate(&p, Profile::Oracle, ctx),
        "Tracing must not change enforcement"
    );
    let trace = trace.unwrap();
    assert_eq!(trace.steps.len(), p.workflow.len());
    assert_eq!(trace.source_hash, p.source_hash);
    assert_eq!(trace.ir_hash, p.ir_hash);
    for (node, step) in p.workflow.iter().zip(&trace.steps) {
        assert_eq!(step.node_id, node.id);
        assert_eq!(
            step.visited,
            !matches!(step.status, S::Inactive | S::Skipped)
        );
    }
    (p, decision, trace)
}
fn states(trace: &WorkflowTrace) -> Vec<S> {
    trace.steps.iter().map(|s| s.status).collect()
}

#[test]
fn straight_execution_records_exact_steps_and_halt() {
    let body = "set_cap(ctx,\"10\",\"USDC\")?; cap_per_transaction(ctx,\"1\",\"USDC\")?; cap_per_transaction(ctx,\"0.5\",\"USDC\")?; Ok(())";
    let (_, d, trace) = traced(body, &context());
    assert_eq!(d.code, "PURCHASE_CAP_EXCEEDED");
    assert!(trace.complete);
    assert_eq!(
        states(&trace),
        vec![S::Passed, S::Passed, S::Failed, S::Skipped]
    );
    let (_, d, trace) = traced(
        body,
        &Context {
            amount_units: 100_000,
            ..context()
        },
    );
    assert_eq!(d.outcome, "pass");
    assert_eq!(states(&trace), vec![S::Passed; 4]);
}

#[test]
fn hoisted_caps_do_not_claim_earlier_source_steps_ran() {
    let (_, d, trace) = traced(
        "require_merchant(ctx,\"other\")?; set_cap(ctx,\"0.5\",\"USDC\")?; Ok(())",
        &context(),
    );
    assert_eq!(d.code, "POLICY_CAP_EXCEEDED");
    assert_eq!(states(&trace), vec![S::Skipped, S::Failed, S::Skipped]);

    let (_, d, trace) = traced(
        "if ctx.amount_units > 0 { return Ok(()); } require_merchant(ctx,\"other\")?; set_cap(ctx,\"10\",\"USDC\")?; Ok(())",
        &context(),
    );
    assert_eq!(d.outcome, "pass");
    assert_eq!(
        states(&trace),
        vec![S::Passed, S::Skipped, S::Passed, S::Skipped]
    );
}

#[test]
fn purchase_tiers_preflight_is_traced_even_after_early_return() {
    let body = "if ctx.amount_units > 0 { return Ok(()); } cap_purchase_tiers(ctx,\"1\",2,\"USDC\")?; Ok(())";
    let ctx = Context {
        purchase_counts: Some(vec![0; 40]),
        ..context()
    };
    let (_, d, trace) = traced(body, &ctx);
    assert_eq!(d.outcome, "pass");
    assert_eq!(states(&trace), vec![S::Passed, S::Passed, S::Skipped]);
    let mut ctx = ctx;
    ctx.purchase_counts.as_mut().unwrap()[0] = 2;
    let (_, d, trace) = traced(body, &ctx);
    assert_eq!(d.code, "PURCHASE_TIER_EXCEEDED");
    assert_eq!(states(&trace), vec![S::Skipped, S::Failed, S::Skipped]);
}

#[test]
fn custom_blocks_report_only_the_selected_path() {
    let body = "let note = \"🌱\"; if ctx.amount_units > 1000000 { require_user_input(ctx,\"Review\").await?; } Ok(())";
    let (p, d, trace) = traced(body, &context());
    assert_eq!(p.workflow.len(), 2);
    assert_eq!(p.workflow[0].kind, "custom");
    assert_eq!(d.outcome, "pass");
    assert_eq!(states(&trace), vec![S::Passed, S::Passed]);
    let (_, d, trace) = traced(
        body,
        &Context {
            amount_units: 2_000_000,
            ..context()
        },
    );
    assert_eq!(d.outcome, "awaiting_input");
    assert!(!trace.complete);
    assert_eq!(states(&trace), vec![S::AwaitingInput, S::Inactive]);
}

#[test]
fn jev_pending_and_owner_resume_replace_the_trace() {
    let body = "set_cap(ctx,\"10\",\"USDC\")?; check_preference(ctx,\"Primary 🌱 evidence?\",0.40,0.85).await?; Ok(())";
    let (p, d, trace) = traced(body, &context());
    assert_eq!(d.code, "SEMANTIC_EVIDENCE_REQUIRED");
    assert_eq!(p.workflow[1].kind, "preference");
    assert!(!trace.complete);
    assert_eq!(states(&trace), vec![S::Passed, S::Verifying, S::Inactive]);
    assert!(d.source_start.is_some());
    let mut ctx = context();
    ctx.confidence.insert(
        semantic_evidence_key("Primary 🌱 evidence?"),
        allowit_sdk::ConfidenceInterval {
            lower_bps: 7000,
            upper_bps: 7000,
        },
    );
    let (_, d, trace) = traced(body, &ctx);
    assert_eq!(
        states(&trace),
        vec![S::Passed, S::AwaitingInput, S::Inactive]
    );
    ctx.answers.insert(d.input_key.unwrap(), true);
    let (_, d, trace) = traced(body, &ctx);
    assert_eq!(d.outcome, "pass");
    assert!(trace.complete);
    assert_eq!(states(&trace), vec![S::Passed; 3]);
}

#[test]
fn invalid_context_never_marks_a_node_visited() {
    let (_, _, trace) = traced(
        "set_cap(ctx,\"10\",\"USDC\")?; Ok(())",
        &Context {
            amount_units: 0,
            ..context()
        },
    );
    assert!(trace.complete);
    assert_eq!(states(&trace), vec![S::Skipped; 2]);
}

#[test]
fn trace_identity_comes_from_recompiled_source_not_projection() {
    let mut p = compile(&source("set_cap(ctx,\"10\",\"USDC\")?; Ok(())")).unwrap();
    let canonical = p.workflow.clone();
    p.workflow[0].id = "forged".into();
    p.workflow[0].start = usize::MAX;
    let (_, trace) = evaluate_with_trace(&p, &context());
    assert_eq!(trace.unwrap().steps[0].node_id, canonical[0].id);
    p.source_hash = "forged".into();
    let (d, trace) = evaluate_with_trace(&p, &context());
    assert_eq!(d.code, "INVALID_ARTIFACT");
    assert!(trace.is_none());
}

#[test]
fn wire_trace_is_opt_in_oracle_only_and_bounded_by_workflow() {
    let body = format!(
        "{}Ok(())",
        "cap_per_transaction(ctx,\"1\",\"USDC\")?;".repeat(40)
    );
    let mut request = json!({"operation":"evaluate","source":source(&body),"profile":"oracle","context":context()});
    let normal = process_value(request.clone());
    assert!(normal.get("trace").is_none());
    request["trace"] = json!(true);
    let out = process_value(request.clone());
    assert_eq!(out["decision"], normal["decision"]);
    assert_eq!(out["trace"]["steps"].as_array().unwrap().len(), 41);
    assert_eq!(out["trace"]["source_hash"], out["source_hash"]);
    assert_eq!(out["trace"]["ir_hash"], out["ir_hash"]);
    assert!(out["trace"].to_string().len() < 5000);
    request["profile"] = json!("contract");
    assert_eq!(
        process_value(request.clone())["error"]["code"],
        "INVALID_REQUEST"
    );
    request["trace"] = json!(false);
    assert!(process_value(request.clone()).get("trace").is_none());
    request["trace"] = json!("yes");
    assert_eq!(process_value(request)["error"]["code"], "INVALID_REQUEST");
}
