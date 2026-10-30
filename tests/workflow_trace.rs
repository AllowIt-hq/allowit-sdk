#![cfg(feature = "compiler")]
use allowit_sdk::{
    Context, WorkflowStepStatus as S, WorkflowTrace, compile, evaluate_with_policy_rules,
    evaluate_with_trace_policy_rules, process_value, semantic_evidence_key,
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
        daily_spent_units: Some(0),
        spent_day: Some(0),
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
    let mut ctx = ctx.clone();
    ctx.policy_state = Some(allowit_sdk::TrustedPolicyState::default());
    ctx.rule_facts = Some(allowit_sdk::PolicyRuleFacts {
        operation_id: "workflow-trace".into(),
        grant_available: true,
        amount_units: ctx.amount_units,
        asset_id: "SAC".into(),
        recipient: "recipient".into(),
        now: ctx.now,
        expense: Some(allowit_sdk::RuleExpense {
            spent_units: ctx.amount_units,
            gas_units: 0,
        }),
    });
    let (decision, trace) = evaluate_with_trace_policy_rules(&p, &ctx);
    assert_eq!(
        decision,
        evaluate_with_policy_rules(&p, &ctx),
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
    let body = "allowit::day_total_cap(10_000_000)?; allowit::transaction_cap(1_000_000)?; allowit::transaction_cap(500_000)?; Ok(())";
    let (_, d, trace) = traced(body, &context());
    assert_eq!(d.code, "TRANSACTION_CAP_EXCEEDED");
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
fn canonical_calls_respect_source_order_and_unreached_caps_do_not_hoist() {
    let (_, d, trace) = traced(
        "allowit::recipients(&[\"other\"])?; allowit::day_total_cap(500_000)?; Ok(())",
        &context(),
    );
    assert_eq!(d.code, "ADDRESS_NOT_ALLOWED");
    assert_eq!(states(&trace), vec![S::Failed, S::Skipped, S::Skipped]);
    let (p, d, trace) = traced(
        "if ctx.amount_units>0 { return Ok(()); } allowit::transaction_cap(1)?; Ok(())",
        &context(),
    );
    assert_eq!(d.outcome, "pass");
    let cap = p
        .workflow
        .iter()
        .position(|step| step.name == "transaction_cap")
        .unwrap();
    assert!(!trace.steps[cap].visited);
}
#[test]
fn retired_tier_helpers_are_rejected_even_after_an_early_return() {
    assert!(compile(&source("if ctx.amount_units>0 {return Ok(());} allowit::cap_purchase_tiers(ctx,\"1\",2,\"USDC\")?; Ok(())")).is_err());
}

#[test]
fn custom_blocks_report_only_the_selected_path() {
    let body = "let note = \"🌱\"; if ctx.amount_units > 1000000 { allowit::require_user_input(ctx,\"Review\").await?; } Ok(())";
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
    let body = "allowit::day_total_cap(10_000_000)?; allowit::v1::prelude::check_preference(ctx,\"Primary 🌱 evidence?\",0.40,0.85).await?; Ok(())";
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
        "allowit::day_total_cap(10_000_000)?; Ok(())",
        &Context {
            original_intent: "x".repeat(16_385),
            ..context()
        },
    );
    assert!(trace.complete);
    assert_eq!(states(&trace), vec![S::Skipped; 2]);
}

#[test]
fn trace_identity_comes_from_recompiled_source_not_projection() {
    let mut p = compile(&source("allowit::day_total_cap(10_000_000)?; Ok(())")).unwrap();
    let canonical = p.workflow.clone();
    p.workflow[0].id = "forged".into();
    p.workflow[0].start = usize::MAX;
    let (_, trace) = evaluate_with_trace_policy_rules(&p, &trusted_context());
    assert_eq!(trace.unwrap().steps[0].node_id, canonical[0].id);
    p.source_hash = "forged".into();
    let (d, trace) = evaluate_with_trace_policy_rules(&p, &trusted_context());
    assert_eq!(d.code, "INVALID_ARTIFACT");
    assert!(trace.is_none());
}

#[test]
fn wire_trace_is_opt_in_oracle_only_and_bounded_by_workflow() {
    let body = format!("{}Ok(())", "allowit::transaction_cap(None)?;".repeat(40));
    let mut request = json!({"operation":"evaluate","source":source(&body),"profile":"oracle","context":context()});
    let normal = process_value(request.clone());
    assert!(normal.get("trace").is_none());
    request["trace"] = json!(true);
    let out = process_value(request.clone());
    assert_eq!(out["decision"], normal["decision"]);
    assert_eq!(
        out["trace"]["steps"].as_array().unwrap().len(),
        compile(&source(&body)).unwrap().workflow.len()
    );
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

fn trusted_context() -> Context {
    let mut ctx = context();
    ctx.policy_state = Some(allowit_sdk::TrustedPolicyState::default());
    ctx.rule_facts = Some(allowit_sdk::PolicyRuleFacts {
        operation_id: "trace-identity".into(),
        grant_available: true,
        amount_units: ctx.amount_units,
        asset_id: "SAC".into(),
        recipient: "recipient".into(),
        now: ctx.now,
        expense: Some(allowit_sdk::RuleExpense {
            spent_units: ctx.amount_units,
            gas_units: 0,
        }),
    });
    ctx
}
