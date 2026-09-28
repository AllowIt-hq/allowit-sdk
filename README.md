# AllowIt SDK

AllowIt policies are a restricted, validated subset of Rust. This repository contains the source compiler, deterministic interpreter, function registry, workflow projection, CLI and stdio language server. The same `no_std + alloc` interpreter is used by the contract adapters in `contracts/`.

```rust
use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap(ctx, "100", "USDC")?;
    cap_per_transaction(ctx, "10", "USDC")?;
    allow_actions(ctx, &["research"])?;
    require_merchant(ctx, "research.example")?;
    Ok(())
}
```

## Run

Use Rust 1.98 or later. `Cargo.lock` pins the dependency graph.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo check --locked --no-default-features --lib
cargo run --locked -- compile examples/research.rs
cargo run --locked -- evaluate examples/research.rs examples/context.json oracle
cargo run --locked -- evaluate examples/approval.rs examples/context.json contract
cargo run --locked -- workflow examples/approval.rs
cargo run --locked -- evaluate examples/green-investments.rs examples/green-context.json oracle
cargo run --locked -- registry
cargo run --locked -- lsp
```

CLI output is JSON. A failed compile or malformed request exits unsuccessfully. A completed evaluation prints its `pass`, `fail` or non-terminal `awaiting_input` decision; callers must inspect that decision before authorizing an action.

For normal Rust type-checking, depend on this package with the name `allowit`:

```toml
allowit = { package = "allowit-sdk", path = "../AllowIt-sdk" }
```

The prelude is a type-checking facade, not a replacement for the compiler/interpreter. In particular, the restricted runtime uses checked arithmetic, enforces the allocation outside user code, and identifies input calls by their source location. Executing policy source directly with Rust's default arithmetic settings would not enforce this contract. The facade's user-input function fails closed. Tests compile all three example policies against the facade.

## Supported source and execution

Each file contains an optional `use allowit::prelude::*;` and one `pub async fn evaluate(ctx: &Context) -> PolicyResult`. Version 1 accepts immutable `let` values, `if`/`else`, early `return fail("reason")`, `Ok(())`, booleans, strings, `u64` integers, boolean/comparison operators and checked integer `+ - * / %`. Context fields expose amount, allocation, spent, action, merchant, recipient, token, network and evaluation time. Confidence intervals expose `lower_bps` and `upper_bps`.

Every function and every branch is validated, including unreachable code. Unknown syntax, imports, macros, attributes, mutation, shadowing, loops, recursion, arbitrary method calls, I/O, unsafe code and unchecked/discarded function results are rejected. Source is limited to 32 KiB and 1,024 syntax tokens (including opening and closing delimiters), IR to 2,048 nodes and semantic nesting to 48 levels. The total token budget never resets across statements or groups. Before invoking `syn`, a token preflight checks the exact function signature and permits only simple `u64`, `bool`, `&str` or `ConfidenceInterval` local annotations. Type declarations, casts, closures and qualified type expressions are rejected before Rust's recursive type parser runs. An iterative token-tree walk bounds actual delimiter depth to 32 and each statement/header to 256 tokens, 96 punctuation operators and 32 control prefixes, with at most 32 `else` branches in a policy. Only the function body and statement-level `if`/`else` bodies may contain code blocks; conditional expressions and semicolons inside expression groups are unsupported. Flat guard statements have independent budgets. Parentheses and brackets contribute to their enclosing expression budget, so shallow postfix call/index/cast chains cannot hide a deep AST. String contents and comments do not alter structural depth. UTF-8 source must not contain a leading byte-order mark.

All monetary values in the portable core are micro-USDC (six decimal places). Decimal limit strings are converted exactly; excess precision, zero, negatives and overflow are rejected. Rail adapters must verify the asset and normalize its actual decimal precision without rounding. The Stellar adapter therefore rejects sub-micro-unit dust when converting its seven-decimal asset. Version 1 supports only the bound USDC asset, not arbitrary tokens described as USDC.

`set_cap` is a configuration declaration: it may occur at most once, at the top level, with literal amount and token. It is enforced before user control flow, including when placed after an early return. The host allocation and spent amount are always enforced independently. Policies can restrict this budget but cannot increase it.

The predefined registry contains `set_cap`, `cap_per_transaction`, `allow_actions`, `require_merchant`, `require_recipient`, `confidence`, `semantic`, `context_u64`, `require_user_input` and `fail`. Help, signatures and workflow labels come from that registry.

Pass and fail are the only terminal outcomes. In the oracle profile, reaching `require_user_input(...).await?` returns a non-authorizing `awaiting_input` decision with an input key and prompt. The engine must authenticate the owner, bind the exact policy/action/evidence snapshot, persist the suspension and answers, reject replay, enforce expiry/revocation and recheck fresh budgets before executing. The SDK does not authenticate a plain `answers` map. Each key binds the policy source digest, call location and prompt; distinct calls cannot share an approval by merely repeating the prompt.

In the contract profile, every reached user-input call fails with `USER_INPUT_REQUIRED`, even when an answer is present. An oracle approval does not change this rule. Confidence intervals must come from trusted, bound evidence. Missing evidence fails, malformed intervals fail, and no score is invented. Valid bounds are ordered integers between 0 and 10,000 basis points. A self-reported model score is not a calibrated interval.

## Preference questions and runtime JSON

`Context.original_intent` preserves up to 16 KiB of the owner's actual instructions. `Context.runtime_context` accepts an object of at most 16 KiB serialized JSON, depth8 and 128 total object/array entries. All available transaction facts and their provenance can travel in that object. They are claims until the host authenticates them. `context_u64(ctx, "key")?` reads a strict non-negative whole number from a top-level key; it never coerces strings, fractions or missing values.

`semantic(ctx, "exact question")?` reads an assessment keyed by lowercase SHA-256 of the exact UTF-8 question. A nonempty original intent is required. Missing evidence produces a terminal SDK failure `SEMANTIC_EVIDENCE_REQUIRED` with `question` and `evidence_key`. The trusted host may intercept that specific result, ask Jev by TypeSafe with the question, original instructions and the complete bound runtime context, authenticate and persist the result, then reevaluate. The SDK never contacts a provider, invents a score or implicitly approves a request. Contract execution requires verified evidence bound to the same mandate/context.

The question hash identifies an evidence slot within one evaluation; it does **not** authenticate evidence or make it reusable. Before populating that slot, the host must bind the provider response to the exact source/IR digests, policy revision, owner, action, amount, recipient, network, token, original intent, complete runtime-context digest and evidence expiry. A change to any bound request field requires fresh applicable evidence. Persisted scores must never be retrieved by question hash alone or copied to another request. If the authenticated provider path is unavailable, preserve the failure for the owner; do not invent an assessment or send context elsewhere.

Jev point scores represented as equal lower/upper basis-point bounds remain **point scores**. They are not calibrated confidence intervals; a real calibrated interval requires supporting provenance. The host owns that distinction and its record. Deterministic constraints still apply independently: `examples/green-investments.rs` rejects an expected-return loss above one **percentage point** (100 basis points), even with the highest preference score. This is distinct from a relative 1% return loss.

The [agent skill](skills/allowit/SKILL.md) includes JSON-context instructions, security boundaries and runnable commands.

## Embedding and wire protocol

Native callers use `process_value(serde_json::Value) -> serde_json::Value` or `process_json(&str) -> String`. The JSON API always compiles source before evaluation and rejects externally supplied executable IR.

Compilation releases its temporary proc-macro source maps after every call so persistent hosts do not retain every edited document. No parser span escapes the SDK. The compiler is intended for standalone native/WASM hosts, not for execution from inside a Rust procedural macro. Hosts must not retain unrelated `proc_macro2::Span` values across compilation calls on the same thread. CLI/JSON error `line` and `column` are one-based Unicode-scalar positions; LSP converts columns to zero-based UTF-16.

```json
{"operation":"compile","source":"pub async fn evaluate(ctx: &Context) -> PolicyResult { Ok(()) }"}
```

Compilation returns `ok: true` and `policy` with `language: "allowit-rust-v1"`, source/IR SHA-256 digests, registry version, optional limit (empty string when uncapped), token, exact source, workflow blocks, call spans and typed IR. Evaluation accepts `operation: "evaluate"`, `source`, `profile: "oracle" | "contract"` and `context` in the shape of `examples/context.json`. It returns `decision.outcome`, `code` and `reason`, plus `prompt` and `input_key` when suspended. Invalid requests return `ok: false` and a typed `error`.

Workflow and call spans are **UTF-16 code-unit offsets** for browser strings and LSP. Internal IR spans use UTF-8 bytes. Predefined top-level calls with literal arguments become function blocks; contiguous custom statements remain exact Custom code, including their internal comments. Nested calls stay inside their custom block and retain individual help spans. The full source is retained byte-for-byte. The visual projection does not execute independently.

Contract adapters use `default-features = false`, `validate_program`, `canonical_ir_hash` and `evaluate_ir`. The contract must authenticate the IR artifact during owner activation and bind it to the action, owner, network, asset, current budget, original intent, runtime-context digest and authority. The `no_std` evaluator validates the complete IR but does not contain the source parser; it does not claim to recompile source on chain. Source digests are owner-bound metadata there. Native `evaluate` recompiles source to reject a forged source/IR correspondence and is available only with the compiler feature.

## WebAssembly

```sh
rustup target add wasm32-unknown-unknown
cargo rustc --locked --release --lib --target wasm32-unknown-unknown --crate-type cdylib
```

The artifact is `target/wasm32-unknown-unknown/release/allowit_sdk.wasm`. It imports no filesystem or network capabilities. The host must enforce memory and execution-time limits. A WebAssembly trap aborts Rust cleanup and can leave the stack or allocator state unusable: discard the instance after any trap and create a new instance for subsequent requests. Never retry against a trapped instance or interpret a trap as approval.

- `alloc(len: u32) -> u32`: allocate an input buffer.
- `process(ptr: u32, len: u32) -> u64`: process UTF-8 JSON, returning output pointer in the high 32 bits and output byte length in the low 32 bits.
- `dealloc(ptr: u32, len: u32)`: free the input and output once each, with their exact original lengths.

## Language server

`allowit lsp` uses standard `Content-Length` JSON-RPC framing on stdin/stdout. It implements initialization, full-document synchronization, compiler diagnostics, registry hover help, completion, shutdown and a custom `allowit/workflow` request. That request accepts `textDocument.uri` or explicit `source` and returns the same compiler artifact as the CLI/API. Diagnostics and workflow data cannot disagree with a separate visualization parser because they share the compiler.

## Verification boundaries

Tests cover exact limits, cap bypass attempts, arithmetic overflow, syntax rejection, forged IR, deterministic oracle/contract decisions, source spans, workflow retention, confidence failures, approval continuation keys, contract input failure, facade type-checking, LSP behavior and a seeded bounded mutation corpus. `contracts/` contains native rail adapters and their own build/test instructions. Compilation or a local contract test is not evidence that a program has been deployed or that funds moved on a public network.

## Readable amounts and comparisons

These helpers require source compiler revision `65a9bfe12f5abcbc7786a37b15f7f7a34f708dbd` or later. Older compilers reject helper source; the existing IR registry remains compatible because helpers lower to its existing operations. Hosts must pin the compiler artifact.

Use decimal strings in policy source; runtime context remains JSON with integer base units. The compiler checks these helpers and lowers them to the existing integer/comparison IR used by every rail. No floats or rounding are involved. Keep the `?` on each helper.

| Helper | Meaning |
| --- | --- |
| `usdc("25.50")?` | 25.50 USDC, exactly 25,500,000 units; up to six decimals. Zero is allowed in comparisons. |
| `percent("85.25")?` | 85.25%, exactly 8,525 basis points; 0–100 with up to two decimals. |
| `amount_at_most(ctx, "25.50")?` | Whether this purchase is at or below 25.50 USDC, including equality. |
| `within_percentage_points(candidate, benchmark, "1")?` | Whether a candidate return is at most one percentage point below the benchmark. Both values are immutable integer variables or literals in basis points; 4% versus 5% passes. Higher returns pass. |

These helpers do not create an allowance. Use `set_cap` for the total allocation and `cap_per_transaction` for a per-purchase limit, with positive decimal strings. USDC has six policy decimals; Testnet uses its bound six-decimal test token. Other token precisions are not inferred from symbols. Rail adapters bind the actual asset and reject precision loss.

```rust
if !amount_at_most(ctx, "25.50")? {
    require_user_input(ctx, "Approve this purchase above 25.50 USDC?").await?;
}
let fit = semantic(ctx, "Is there primary evidence supporting this purchase?")?;
if fit.lower_bps < percent("85")? {
    return fail("The evidence does not meet your threshold");
}
```

Decimal helper arguments must be string literals. Excess decimal places, signs, exponent notation, separators and overflow are compile errors. Bind candidate and benchmark returns to variables before comparing them. Their values are claims until authenticated; comparison helpers do not establish provenance. Helpers inside custom logic keep their exact source and function tips in the workflow.

### Local development and preference thresholds

`local:dev` is a wallet-free oracle network. Contract evaluation rejects it. Local action records are never blockchain settlement.

`check_preference(ctx, "Does the evidence support this preference?", true, "85", true, "40").await?;` is a predefined source helper. Its arguments are the exact question, automatic approval flag and minimum percentage, then automatic denial flag and maximum percentage. Enabled comparisons include equality. Denial must be strictly below approval when both are enabled. Disable either outcome independently; with both disabled, every reached check asks the owner without calling Jev. Missing or invalid evidence fails closed. The question, original intent and runtime JSON feed the host's Jev assessment. The one estimated field is `preference_fit: number` in `[0,1]`, a point score rather than calibrated confidence. The host rounds down to four decimal places and supplies equal integer basis-point bounds. Hard spending rules still apply.

The helper lowers to the existing semantic, branch, failure and user-input IR operations; no new contract opcode is added. Oracle input suspends for an authenticated answer; a reached input call fails in contracts. Top-level helpers get a distinct Jev workflow block with editable literal settings; nested calls remain inside their conditional custom code. Existing top-level `let fit = semantic(...)` calls also get their own workflow item, while the branches using the result remain custom code.
