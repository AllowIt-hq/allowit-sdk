use allowit_sdk::{Context, Profile, canonical_ir_hash, compile, evaluate, process_value};
use serde_json::json;
fn source(body: &str) -> String {
    format!("pub async fn evaluate(ctx: &Context) -> PolicyResult {{ {body} }}")
}
fn context() -> Context {
    serde_json::from_str(include_str!("../examples/context.json")).unwrap()
}

#[test]
fn forged_ir_cannot_replace_the_compiled_source() {
    let mut policy = compile(&source("return fail(\"No spending\");")).unwrap();
    let weaker = compile(&source("Ok(())")).unwrap();
    policy.ir = weaker.ir;
    policy.ir_hash = canonical_ir_hash(&policy.ir).unwrap();
    assert_eq!(
        evaluate(&policy, Profile::Oracle, &context()).code,
        "INVALID_ARTIFACT"
    );
    let request = process_value(
        json!({"operation":"evaluate","source":source("Ok(())"),"profile":"oracle","context":context(),"ir":policy.ir}),
    );
    assert_eq!(request["error"]["code"], "INVALID_REQUEST");
}
#[test]
fn all_syn_escape_hatches_are_rejected() {
    let constructs = [
        "let x = move || true; Ok(())",
        "let x = async { true }; Ok(())",
        "let x = [1; 100]; Ok(())",
        "let x = ctx as *const Context; Ok(())",
        "let x = (1,2); Ok(())",
        "let x = [1][0]; Ok(())",
        "let x = &ctx; Ok(())",
        "let x = &mut ctx; Ok(())",
        "let x = *ctx; Ok(())",
        "let x = -1; Ok(())",
        "let x = 1u32; Ok(())",
        "let x = 1.0; Ok(())",
        "let x = b\"secret\"; Ok(())",
        "let x = 'x'; Ok(())",
        "ctx.amount_units = 0; Ok(())",
        "ctx.amount_units += 0; Ok(())",
        "let x = ctx.clone(); Ok(())",
        "let x = if true { 1 } else { 2 }; Ok(())",
        "let x = match true { true=>1,false=>2 }; Ok(())",
        "for x in 0..10 {} Ok(())",
        "'label: { break 'label; } Ok(())",
        "const X:u64=1; Ok(())",
        "fn helper() {} Ok(())",
        "use std::fs; Ok(())",
        "let x = Some(1); Ok(())",
        "let x = Context {}; Ok(())",
        "let x = 1; let x = 2; Ok(())",
        "let x = 1; if true { let x = 2; } Ok(())",
        "let (x,y) = (1,2); Ok(())",
        "let ref x = 1; Ok(())",
        "let x; Ok(())",
        "let x = 1 else { return Ok(()); }; Ok(())",
        "#[cfg(any())] unknown(); Ok(())",
        "let x = 1 << 3; Ok(())",
        "let x = evaluate(ctx); Ok(())",
        "if false { return unknown(); } Ok(())",
        "if false { let ctx = 0; } Ok(())",
    ];
    for construct in constructs {
        assert!(compile(&source(construct)).is_err(), "accepted {construct}");
    }
}
#[test]
fn nested_comments_and_strings_do_not_hide_parser_depth() {
    let deep = format!(
        "{}Ok(()){}",
        "if true { let punctuation = \"}}}}}}\";".repeat(150),
        "}".repeat(150)
    );
    assert!(compile(&source(&deep)).is_err());
    let comments = format!("{}{} Ok(())", "/*".repeat(150), "*/".repeat(150));
    // Comment nesting is handled iteratively by the lexer and does not create AST depth.
    assert!(compile(&source(&comments)).is_ok());
    assert!(compile(&source("let message = r###\"'))]} 👋\"###; Ok(())")).is_ok());
    assert!(
        compile(&source(&format!(
            "if {}true {{ return fail(\"No\"); }} Ok(())",
            "!".repeat(4000)
        )))
        .is_err()
    );
    assert!(
        compile(&source(&format!(
            "let total = {}1; Ok(())",
            "1 + ".repeat(4000)
        )))
        .is_err()
    );
    assert!(
        compile(&source(&format!(
            "{} Ok(())",
            "if true { return fail(\"No\"); } else ".repeat(500)
        )))
        .is_err()
    );
}
#[test]
fn malformed_confidence_never_passes_even_if_unused() {
    let p = compile(&source("Ok(())")).unwrap();
    let mut ctx = context();
    ctx.confidence.get_mut("safety").unwrap().lower_bps = 10001;
    assert_eq!(evaluate(&p, Profile::Oracle, &ctx).code, "INVALID_EVIDENCE");
}
#[test]
fn deterministic_boundary_vectors_match_oracle_and_contract() {
    let p=compile(&source("set_cap(ctx, \"100\", \"USDC\")?; cap_per_transaction(ctx, \"10\", \"USDC\")?; if ctx.amount_units % 2 == 1 { return fail(\"Even micro-units only\"); } Ok(())")).unwrap();
    for amount in [0, 1, 2, 9_999_999, 10_000_000, 10_000_001, u64::MAX] {
        for spent in [0, 90_000_000, 99_999_999, 100_000_000, u64::MAX] {
            let mut ctx = context();
            ctx.amount_units = amount;
            ctx.spent_units = spent;
            assert_eq!(
                evaluate(&p, Profile::Oracle, &ctx),
                evaluate(&p, Profile::Contract, &ctx)
            );
        }
    }
}
#[test]
fn bounded_token_mutations_never_panic_and_successful_compiles_are_deterministic() {
    let original = source(
        "set_cap(ctx, \"100\", \"USDC\")?; if ctx.action == \"research\" { require_merchant(ctx, \"research.example\")?; } Ok(())",
    );
    let mut seed = 0x5eed_u64;
    let tokens = [
        "unsafe", "/*", "*/", "{", "}", "\"", "???", "👋", "", "await", ";", "cfg", "\n", "0", "_",
        "😀",
    ];
    for _ in 0..600 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let at = (seed as usize) % original.len();
        let mut candidate = original.clone();
        candidate.insert_str(at, tokens[(seed >> 32) as usize % tokens.len()]);
        let first = std::panic::catch_unwind(|| compile(&candidate))
            .expect("compiler panicked on bounded input");
        if let Ok(policy) = first {
            assert_eq!(compile(&candidate).unwrap(), policy);
            assert_eq!(
                evaluate(&policy, Profile::Oracle, &context()),
                evaluate(&policy, Profile::Oracle, &context())
            );
        }
    }
}

#[test]
fn postfix_index_cast_and_prefix_chains_are_rejected_on_a_one_megabyte_stack() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            for body in [
                format!("let y = f{}; Ok(())", "()".repeat(15000)),
                format!("let y = f{}; Ok(())", "({})".repeat(7000)),
                format!("let y = x{}; Ok(())", "[0]".repeat(10000)),
                format!("let y = 1{}; Ok(())", " as u64".repeat(4500)),
                format!("let y = {}true; Ok(())", "!".repeat(1000)),
                format!("{}Ok(())", "return ".repeat(1000)),
                format!("let y = {}1; Ok(())", "if true {} = ".repeat(1000)),
                format!("let y = {}1; Ok(())", "if true {} + ".repeat(2000)),
                format!("let y = x{}; Ok(())", " = [0;0]".repeat(3500)),
                format!("let y = 1{}; Ok(())", " + [0;0]".repeat(2000)),
                format!("let y = f{}; Ok(())", "([0;0])".repeat(3500)),
                format!("let y = x{}; Ok(())", ".f([0;0])".repeat(3500)),
                format!(
                    "let a = br\"\\\"; {}{}// \"\nOk(())",
                    "if a {".repeat(1000),
                    "}".repeat(1000)
                ),
                format!(
                    "let a = cr\"\\\"; {}{}// \"\nOk(())",
                    "if a {".repeat(1000),
                    "}".repeat(1000)
                ),
            ] {
                assert!(compile(&source(&body)).is_err());
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn flat_guards_and_twelve_clause_conditions_are_not_mistaken_for_nesting() {
    let guards = (0..40)
        .map(|n| format!("if ctx.amount_units == {n} {{ return fail(\"No\"); }}"))
        .collect::<String>();
    assert!(compile(&source(&format!("{guards} Ok(())"))).is_ok());
    let clauses = (0..12)
        .map(|n| format!("ctx.amount_units > {n}"))
        .collect::<Vec<_>>()
        .join(" && ");
    assert!(
        compile(&source(&format!(
            "if {clauses} {{ return fail(\"No\"); }} Ok(())"
        )))
        .is_ok()
    );
}

#[test]
fn global_token_budget_bounds_flat_code_and_mixed_shapes_on_one_megabyte_stack() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let calls = "cap_per_transaction(ctx, \"10\", \"USDC\")?;";
            assert!(compile(&source(&format!("{}Ok(())", calls.repeat(25)))).is_ok());
            let too_many = compile(&source(&format!("{}Ok(())", calls.repeat(200))))
                .expect_err("large flat policies must hit the global token budget");
            assert_eq!(too_many.code, "RESOURCE_LIMIT");
            assert!(too_many.message.contains("1,024"));
            for depth in [1, 8, 16, 31] {
                for clauses in [1, 8, 16] {
                    let condition = vec!["ctx.amount_units > 0"; clauses].join(" && ");
                    let body = format!(
                        "{}{}Ok(())",
                        format!("if {condition} {{").repeat(depth),
                        "}".repeat(depth),
                    );
                    // Exercise combinations near both global and local parser limits;
                    // acceptance depends on semantic depth, but compilation must never abort.
                    let _ = compile(&source(&body));
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn unsupported_recursive_types_are_rejected_on_one_megabyte_stack() {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let simple = source("Ok(())");
            let mut invalid_sources = vec![
                format!("pub async fn evaluate(ctx: {}Context) -> PolicyResult {{ Ok(()) }}", "&".repeat(90)),
                format!("type T = {}u8{}; {simple}", "A<".repeat(47), ">".repeat(47)),
                format!("type T = {}u8; {simple}", "*const ".repeat(90)),
                format!("type T = {}u8{}; {simple}", "impl A<".repeat(47), ">".repeat(47)),
                format!("type T = {}u8{}; {simple}", "Box<dyn A<".repeat(23), ">>".repeat(23)),
                format!("type T = {}u8; {simple}", "fn() -> ".repeat(47)),
                source(&format!("let a: {}u8 = 1; Ok(())", "&".repeat(90))),
                source(&format!("let a = <{}u8{}; Ok(())", "A<".repeat(47), ">".repeat(48))),
                source(&format!("if || -> {}u8 {{ 1 }} {{ return fail(\"No\"); }} Ok(())", "&".repeat(90))),
                source(&format!("if |x| -> {}u8 {{ 1 }} {{ return fail(\"No\"); }} Ok(())", "&".repeat(90))),
            ];
            invalid_sources.push(format!("pub async fn evaluate(ctx: {}Context) -> PolicyResult {{ Ok(()) }}", "&".repeat(29)));
            for candidate in invalid_sources {
                assert!(compile(&candidate).is_err());
            }
            let valid = source("let label: &str = \"research\"; let score: ConfidenceInterval = confidence(ctx, \"safety\")?; let matches: bool = ctx.action == label || ctx.action == \"investment\"; if !matches || score.upper_bps < 10 { return fail(\"No\"); } Ok(())");
            assert!(compile(&valid).is_ok());
        })
        .unwrap()
        .join()
        .unwrap();
}
