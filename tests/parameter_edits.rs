use allowit_sdk::{
    ConfidenceInterval, Context, Profile, compile, evaluate, process_value, semantic_evidence_key,
};
use serde_json::{Value, json};

fn source(body: &str) -> String {
    format!("pub async fn evaluate(ctx: &Context) -> PolicyResult {{\n{body}\n}}\n")
}

fn step_id(source: &str, name: &str, index: usize) -> String {
    compile(source)
        .unwrap()
        .workflow
        .into_iter()
        .filter(|step| step.name == name)
        .nth(index)
        .unwrap()
        .id
}

fn settings(id: &str) -> Value {
    json!({
        "step_id": id,
        "auto_approve": true,
        "approve_percent": "90",
        "auto_deny": true,
        "deny_percent": "30"
    })
}

fn preference(source: &str, settings: Value) -> Value {
    process_value(json!({"operation":"edit_preference", "source":source, "settings":settings}))
}

fn thresholds(source: &str, step_id: &str, values: Value) -> Value {
    process_value(json!({
        "operation":"edit_score_thresholds", "source":source,
        "step_id":step_id, "values":values
    }))
}

fn edited(response: &Value) -> &str {
    assert_eq!(response["ok"], true, "{response}");
    let source = response["source"].as_str().unwrap();
    let policy = compile(source).unwrap();
    assert_eq!(response["policy"]["source"], source);
    assert_eq!(response["policy"]["source_hash"], policy.source_hash);
    assert_eq!(response["policy"]["ir_hash"], policy.ir_hash);
    source
}

fn rejected(response: Value) {
    assert_eq!(response["ok"], false, "{response}");
    assert!(response["error"]["code"].is_string(), "{response}");
    assert!(response.get("source").is_none(), "{response}");
    assert!(response.get("policy").is_none(), "{response}");
}

fn context(question: &str, lower: u64, upper: u64) -> Context {
    let mut context: Context =
        serde_json::from_str(include_str!("../examples/green-context.json")).unwrap();
    context.confidence.insert(
        semantic_evidence_key(question),
        ConfidenceInterval {
            lower_bps: lower,
            upper_bps: upper,
        },
    );
    context
}

#[test]
fn current_helper_preserves_unicode_comments_and_unrelated_literal_text() {
    let original = source(
        r#"// 🌱 café: check_preference(ctx, "Research", 0.40, 0.85).await?;
let note = "0.40, 0.85; fit.lower_bps < 8500 🧪";
check_preference(ctx, "Research 🧪, 0.40, 0.85", /* deny */ 0.40, /* approve */ 0.85).await?;
Ok(())"#,
    );
    let response = preference(
        &original,
        settings(&step_id(&original, "check_preference", 0)),
    );
    let expected = original.replace(
        "/* deny */ 0.40, /* approve */ 0.85",
        "/* deny */ 0.3000, /* approve */ 0.9000",
    );
    assert_eq!(edited(&response), expected);
    assert_ne!(
        compile(&original).unwrap().ir_hash,
        response["policy"]["ir_hash"]
    );
}

#[test]
fn identical_questions_are_targeted_by_step_id_for_both_helpers() {
    for call in [
        "check_preference(ctx, \"Same question\", 0.40, 0.85).await?;",
        "check_preference(ctx, \"Same question\", true, \"85\", true, \"40\").await?;",
    ] {
        let original = source(&format!(
            "{call}\n// Keep the first occurrence.\n{call}\nOk(())"
        ));
        let first = step_id(&original, "check_preference", 0);
        let second = step_id(&original, "check_preference", 1);
        assert_ne!(first, second);
        let response = preference(&original, settings(&second));
        let replacement = if call.contains("0.40") {
            call.replace("0.40, 0.85", "0.3000, 0.9000")
        } else {
            call.replace(
                "true, \"85\", true, \"40\"",
                "true, \"90.00\", true, \"30.00\"",
            )
        };
        let expected = source(&format!(
            "{call}\n// Keep the first occurrence.\n{replacement}\nOk(())"
        ));
        assert_eq!(edited(&response), expected);
    }
}

#[test]
fn legacy_helper_changes_only_selected_arguments() {
    let original = source(
        r#"// Décision 🌱; unrelated values: true, "85", true, "40".
check_preference(ctx, "Research 🌱", true /* A */, "85" /* B */, true /* C */, "40" /* D */).await?;
Ok(())"#,
    );
    let mut values = settings(&step_id(&original, "check_preference", 0));
    values["auto_approve"] = json!(false);
    values["approve_percent"] = json!("91.25");
    values["deny_percent"] = json!("2.50");
    let response = preference(&original, values);
    assert_eq!(
        edited(&response),
        original.replace(
            "true /* A */, \"85\" /* B */, true /* C */, \"40\" /* D */",
            "false /* A */, \"91.25\" /* B */, true /* C */, \"2.50\" /* D */"
        )
    );
}

#[test]
fn current_auto_and_none_arguments_can_enable_or_disable_each_outcome() {
    let original =
        source("check_preference(ctx, \"Research\", auto(\"deny\"), None).await?;\nOk(())");
    let mut values = settings(&step_id(&original, "check_preference", 0));
    values["auto_deny"] = json!(false);
    values["approve_percent"] = json!("100");
    let response = preference(&original, values);
    assert_eq!(
        edited(&response),
        original.replace("auto(\"deny\"), None", "None, 1.0000")
    );
    let updated = edited(&response);
    let mut values = settings(&step_id(updated, "check_preference", 0));
    values["auto_approve"] = json!(false);
    values["auto_deny"] = json!(false);
    let disabled = preference(updated, values);
    let policy = compile(edited(&disabled)).unwrap();
    let mut context = context("Research", 5000, 5000);
    context.confidence.clear();
    assert_eq!(
        evaluate(&policy, Profile::Oracle, &context).outcome,
        "awaiting_input"
    );
}

#[test]
fn raw_semantic_bounds_preserve_operators_and_unrelated_source() {
    let original = source(
        r#"// λ 🧪: fit.lower_bps < 8500; percent("40")?
let note = "fit.lower_bps < 8500; fit.upper_bps <= percent(\"40\")?";
let fit = semantic(ctx, "Research 🧪")?;
if fit.upper_bps <= percent("40")? { return fail("No"); }
if fit.lower_bps < 8500 { return fail("Uncertain"); }
Ok(())"#,
    );
    let policy = compile(&original).unwrap();
    let step = policy
        .workflow
        .iter()
        .find(|step| step.name == "semantic")
        .unwrap();
    assert_eq!(step.score_thresholds.len(), 2);
    assert_eq!(step.score_thresholds[0].field, "upper_bps");
    assert_eq!(step.score_thresholds[0].operator, "<=");
    assert_eq!(step.score_thresholds[0].value_bps, 4000);
    assert_eq!(step.score_thresholds[1].field, "lower_bps");
    assert_eq!(step.score_thresholds[1].operator, "<");
    let response = thresholds(&original, &step.id, json!([3000, 9000]));
    let expected = original
        .replace(
            "if fit.upper_bps <= percent(\"40\")?",
            "if fit.upper_bps <= percent(\"30.00\")?",
        )
        .replace("if fit.lower_bps < 8500", "if fit.lower_bps < 9000");
    assert_eq!(edited(&response), expected);
}

#[test]
fn raw_semantic_edits_do_not_confuse_identical_questions_or_other_variables() {
    let original = source(
        r#"let first = semantic(ctx, "Same")?;
let second = semantic(ctx, "Same")?;
if first.lower_bps < 8500 || second.lower_bps < 8500 { return fail("No"); }
if second.upper_bps > percent("95")? { return fail("No"); }
Ok(())"#,
    );
    let response = thresholds(
        &original,
        &step_id(&original, "semantic", 1),
        json!([9000, 10000]),
    );
    assert_eq!(
        edited(&response),
        original
            .replace("second.lower_bps < 8500", "second.lower_bps < 9000")
            .replace(
                "second.upper_bps > percent(\"95\")?",
                "second.upper_bps > percent(\"100.00\")?"
            )
    );
}

#[test]
fn stale_step_ids_and_wrong_operation_targets_are_rejected() {
    let original = source(
        "check_preference(ctx, \"Research\", 0.40, 0.85).await?;\nlet fit = semantic(ctx, \"Other\")?;\nif fit.lower_bps < 8000 { return fail(\"No\"); }\nOk(())",
    );
    let helper_id = step_id(&original, "check_preference", 0);
    let semantic_id = step_id(&original, "semantic", 0);
    let changed = format!("// Unrelated source revision 🌱\n{original}");
    rejected(preference(&changed, settings(&helper_id)));
    rejected(thresholds(&changed, &semantic_id, json!([9000])));
    rejected(preference(&original, settings(&semantic_id)));
    rejected(thresholds(&original, &helper_id, json!([9000])));
    rejected(preference(&original, settings("missing")));
    let first = preference(&original, settings(&helper_id));
    rejected(preference(edited(&first), settings(&helper_id)));
}

#[test]
fn malformed_or_ambiguous_threshold_updates_are_rejected() {
    let original = source("check_preference(ctx, \"Research\", 0.40, 0.85).await?;\nOk(())");
    let id = step_id(&original, "check_preference", 0);
    for invalid in [
        json!("101"),
        json!("-1"),
        json!("NaN"),
        json!("0.001"),
        json!(90),
        Value::Null,
    ] {
        for field in ["approve_percent", "deny_percent"] {
            let mut values = settings(&id);
            values[field] = invalid.clone();
            rejected(preference(&original, values));
        }
    }
    for deny in ["90", "91"] {
        let mut values = settings(&id);
        values["deny_percent"] = json!(deny);
        rejected(preference(&original, values));
    }
    let mut values = settings(&id);
    values["auto_approve"] = json!("true");
    rejected(preference(&original, values));
    let mut values = settings(&id);
    values["unexpected"] = json!(true);
    rejected(preference(&original, values));

    let raw = source(
        "let fit = semantic(ctx, \"Research\")?;\nif fit.lower_bps < 8500 { return fail(\"No\"); }\nOk(())",
    );
    let id = step_id(&raw, "semantic", 0);
    for invalid in [
        json!([]),
        json!([8500, 9000]),
        json!([10001]),
        json!([-1]),
        json!([0.5]),
        json!(["9000"]),
        Value::Null,
    ] {
        rejected(thresholds(&raw, &id, invalid));
    }
}

#[test]
fn helper_edits_change_only_requested_inclusive_score_boundaries() {
    let original = source(
        "set_cap(ctx, \"25\", \"USDC\")?;\ncheck_preference(ctx, \"Research\", 0.40, 0.85).await?;\nOk(())",
    );
    let before = compile(&original).unwrap();
    let response = preference(
        &original,
        settings(&step_id(&original, "check_preference", 0)),
    );
    let after = compile(edited(&response)).unwrap();
    for (score, old, new) in [
        (0, "fail", "fail"),
        (3000, "fail", "fail"),
        (3001, "fail", "awaiting_input"),
        (4000, "fail", "awaiting_input"),
        (4001, "awaiting_input", "awaiting_input"),
        (8499, "awaiting_input", "awaiting_input"),
        (8500, "pass", "awaiting_input"),
        (8999, "pass", "awaiting_input"),
        (9000, "pass", "pass"),
        (10000, "pass", "pass"),
    ] {
        let context = context("Research", score, score);
        assert_eq!(
            evaluate(&before, Profile::Oracle, &context).outcome,
            old,
            "old {score}"
        );
        assert_eq!(
            evaluate(&after, Profile::Oracle, &context).outcome,
            new,
            "new {score}"
        );
    }
    for policy in [&before, &after] {
        let mut context = context("Research", 10000, 10000);
        context.amount_units = 25_000_000;
        assert_eq!(evaluate(policy, Profile::Oracle, &context).outcome, "pass");
        context.amount_units += 1;
        assert_eq!(
            evaluate(policy, Profile::Oracle, &context).code,
            "POLICY_CAP_EXCEEDED"
        );
        context.amount_units = 1;
        context.confidence.clear();
        assert_eq!(
            evaluate(policy, Profile::Oracle, &context).code,
            "SEMANTIC_EVIDENCE_REQUIRED"
        );
    }
    let wide = context("Research", 8999, 10000);
    assert_eq!(
        evaluate(&after, Profile::Oracle, &wide).outcome,
        "awaiting_input"
    );
    assert_eq!(
        evaluate(&after, Profile::Contract, &wide).code,
        "USER_INPUT_REQUIRED"
    );
}

#[test]
fn raw_bound_edits_preserve_inclusive_runtime_comparisons() {
    let original = source(
        "let fit = semantic(ctx, \"Research\")?;\nif fit.lower_bps < 8500 || fit.upper_bps > percent(\"95\")? { return fail(\"No\"); }\nOk(())",
    );
    let response = thresholds(
        &original,
        &step_id(&original, "semantic", 0),
        json!([9000, 9800]),
    );
    let policy = compile(edited(&response)).unwrap();
    for (lower, upper, expected) in [
        (8999, 9800, "fail"),
        (9000, 9800, "pass"),
        (9000, 9801, "fail"),
    ] {
        for profile in [Profile::Oracle, Profile::Contract] {
            assert_eq!(
                evaluate(&policy, profile, &context("Research", lower, upper)).outcome,
                expected
            );
        }
    }
}

#[test]
fn shadowing_is_rejected_before_any_source_edit() {
    let original = source(
        "let fit = semantic(ctx, \"Research\")?;\nif true { let fit = semantic(ctx, \"Other\")?; if fit.lower_bps < 8500 { return fail(\"No\"); } }\nOk(())",
    );
    assert!(compile(&original).is_err());
    rejected(thresholds(&original, "anything", json!([9000])));
}

#[test]
fn typed_semantic_bindings_and_parenthesized_bounds_remain_editable() {
    let original = source(
        r#"// Typé 🌱, preserve every pair of parentheses.
let fit: ConfidenceInterval = (semantic(ctx, "Research")?);
if ((fit).lower_bps) < ((8_500u64)) { return fail("No"); }
if (fit.upper_bps) > (percent("95")?) { return fail("No"); }
Ok(())"#,
    );
    let response = thresholds(
        &original,
        &step_id(&original, "semantic", 0),
        json!([9000, 9800]),
    );
    assert_eq!(
        edited(&response),
        original
            .replace("((8_500u64))", "((9000))")
            .replace("percent(\"95\")?", "percent(\"98.00\")?")
    );
}

#[test]
fn current_helper_preserves_unchanged_numeric_and_auto_arguments_byte_for_byte() {
    for deny in ["0.40", "0.4000", "auto(/* retain this 🌱 */ \"deny\")"] {
        let original = source(&format!(
            "check_preference(ctx, \"Research\", {deny}, /* change only this */ 0.85).await?;\nOk(())"
        ));
        let mut values = settings(&step_id(&original, "check_preference", 0));
        values["deny_percent"] = json!("40.00");
        let response = preference(&original, values);
        assert_eq!(
            edited(&response),
            original.replace(
                "/* change only this */ 0.85",
                "/* change only this */ 0.9000"
            )
        );
    }
}

#[test]
fn legacy_helper_preserves_unchanged_numeric_argument_bytes() {
    let original = source(
        r##"check_preference(ctx, "Research", true, "85", true, r#"040.00"#).await?;
Ok(())"##,
    );
    let mut values = settings(&step_id(&original, "check_preference", 0));
    values["deny_percent"] = json!("40");
    let response = preference(&original, values);
    assert_eq!(
        edited(&response),
        original.replace("true, \"85\"", "true, \"90.00\"")
    );
}

#[test]
fn unchanged_raw_numeric_and_percentage_bounds_keep_exact_spelling() {
    for numeric in ["8_500u64", "0x2134", "8500"] {
        let original = source(&format!(
            "let fit = semantic(ctx, \"Research\")?;\nif fit.lower_bps < {numeric} || fit.upper_bps > percent(\"095.0\")? {{ return fail(\"No\"); }}\nOk(())"
        ));
        let response = thresholds(
            &original,
            &step_id(&original, "semantic", 0),
            json!([8500, 9800]),
        );
        assert_eq!(
            edited(&response),
            original.replace("\"095.0\"", "\"98.00\"")
        );

        let response = thresholds(
            &original,
            &step_id(&original, "semantic", 0),
            json!([9000, 9500]),
        );
        assert_eq!(
            edited(&response),
            original.replace(
                &format!("fit.lower_bps < {numeric}"),
                "fit.lower_bps < 9000"
            )
        );
    }
}

#[test]
fn no_op_parameter_edits_preserve_source_and_artifact_hashes() {
    let helper = source(
        "check_preference(ctx, \"Research 🌱\", auto(/* retain */ \"deny\"), 0.8500).await?;\nOk(())",
    );
    let mut values = settings(&step_id(&helper, "check_preference", 0));
    values["approve_percent"] = json!("85");
    values["deny_percent"] = json!("40");
    let response = preference(&helper, values);
    assert_eq!(edited(&response), helper);
    assert_eq!(
        compile(&helper).unwrap().source_hash,
        response["policy"]["source_hash"]
    );
    assert_eq!(
        compile(&helper).unwrap().ir_hash,
        response["policy"]["ir_hash"]
    );

    let raw = source(
        "let fit = semantic(ctx, \"Research\")?;\nif fit.lower_bps < 8_500u64 || fit.upper_bps > percent(\"095.0\")? { return fail(\"No\"); }\nOk(())",
    );
    let response = thresholds(&raw, &step_id(&raw, "semantic", 0), json!([8500, 9500]));
    assert_eq!(edited(&response), raw);
    assert_eq!(
        compile(&raw).unwrap().source_hash,
        response["policy"]["source_hash"]
    );
}

fn partial_semantic_edit_is_rejected(body: &str) {
    let original = source(body);
    let policy = compile(&original).unwrap();
    let step = policy
        .workflow
        .iter()
        .find(|step| step.name == "semantic")
        .unwrap();
    assert!(
        step.score_thresholds.is_empty(),
        "A partial list would conceal another use of the same assessment: {body}"
    );
    let response = thresholds(&original, &step.id, json!([9500]));
    assert_eq!(response["error"]["code"], "INVALID_EDIT", "{response}");
    rejected(response);
}

#[test]
fn a_reversed_comparison_prevents_partial_literal_threshold_edits() {
    partial_semantic_edit_is_rejected(
        "let fit = semantic(ctx, \"Research\")?;\nif 5000 <= fit.lower_bps { return Ok(()); }\nif fit.lower_bps < 9000 { return fail(\"No\"); }\nOk(())",
    );
}

#[test]
fn a_computed_bound_prevents_partial_literal_threshold_edits() {
    for other_comparison in [
        "let cutoff = 5000; if fit.lower_bps >= cutoff { return Ok(()); }",
        "if fit.lower_bps >= 4000 + 1000 { return Ok(()); }",
        "if fit.lower_bps + 1000 >= 6000 { return Ok(()); }",
    ] {
        partial_semantic_edit_is_rejected(&format!(
            "let fit = semantic(ctx, \"Research\")?;\n{other_comparison}\nif fit.lower_bps < 9000 {{ return fail(\"No\"); }}\nOk(())"
        ));
    }
}

#[test]
fn an_aliased_assessment_prevents_partial_literal_threshold_edits() {
    for alias in [
        "let alias = fit; if alias.lower_bps >= 5000 { return Ok(()); }",
        "let lower = fit.lower_bps; if lower >= 5000 { return Ok(()); }",
    ] {
        partial_semantic_edit_is_rejected(&format!(
            "let fit = semantic(ctx, \"Research\")?;\n{alias}\nif fit.lower_bps < 9000 {{ return fail(\"No\"); }}\nOk(())"
        ));
    }
}

#[test]
fn unsupported_reads_of_another_assessment_do_not_disable_the_selected_one() {
    let original = source(
        "let selected = semantic(ctx, \"Selected\")?;\nlet other = semantic(ctx, \"Other\")?;\nif 5000 <= other.lower_bps { return fail(\"Other\"); }\nif selected.lower_bps < 9000 { return fail(\"Selected\"); }\nOk(())",
    );
    let response = thresholds(&original, &step_id(&original, "semantic", 0), json!([9500]));
    assert_eq!(
        edited(&response),
        original.replace("selected.lower_bps < 9000", "selected.lower_bps < 9500")
    );
}
