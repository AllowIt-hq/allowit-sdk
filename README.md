# AllowIt SDK

AllowIt policies are a restricted, validated subset of Rust. This repository contains the source compiler, deterministic interpreter, function registry, workflow projection, CLI and stdio language server. The same `no_std + alloc` interpreter is used by the contract adapters in `contracts/`.

The native [Stellar SDK](stellar-rust/README.md) prepares standard x402 v2 USDC
payments from Soroban vaults, verifies detached policy-authority and executor
signatures, and checks exact on-chain receipts. It adds no policy functions and
performs no transport, key-file access, or broadcasting.

```rust
use allowit::v1::prelude::*;

struct PolicyParams {}
fn new() -> PolicyParams { PolicyParams {} }

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::set_cap_per_day(ctx.daily_spent_units, ctx.spent_day, ctx.now, ctx.amount_units, &ctx.token, 100_000_000, "USDC", 6)?;
    allowit::cap_per_transaction(ctx.amount_units, &ctx.token, 10_000_000, "USDC", 6)?;
    allowit::require_merchant(&ctx.merchant, "research.example")?;
    jev::check_preference(
        jev::preference_evidence(ctx, "Does this purchase support the requested research?"),
        "Does this purchase support the requested research?",
        auto("deny"), auto("approve"),
    ).await?;
    Ok(())
}
```

The Go [action CLI](https://github.com/AllowIt-hq/allowit-cli) is a separate client for `allowit show`, `eval`, `exec` and `status`. This repository's Rust CLI is a developer compiler/evaluator tool; use the repository-local Cargo commands below so the two executables are not confused. The app server owns HTTP transport. Hosted-agent control and runtime are optional, outside the MVP; they are not SDK responsibilities. Restricted Rust remains the policy source and enforcement language.

## Policy source and configuration

Use the versioned import, `PolicyParams`, private `new()` and `_execute` shown above. Keep limits inline. Immutable configuration accepts inline literals: `u64`, `bool`, `&'static str` and bounded `&'static [&'static str]` lists. Use sequential guards with authenticated primitives.

The [policy template](templates/policy/README.md) validates Rust and restricted source. Immutable configuration changes require a new approved artifact. Native mutable limits use:

```rust
struct PolicyParams {
    daily_limit: OwnerLimit,
    action_limit: OwnerLimit,
}
fn new() -> PolicyParams {
    PolicyParams {
        daily_limit: allowit::owner_limit("native_daily_limit", 5_000_000),
        action_limit: allowit::owner_limit("native_action_limit", 1_000_000),
    }
}
```

`allowit::stored_limit(ctx, params.action_limit)?` reads adapter-verified `Context.native_policy_storage`. Only these two fields work. JSON and portable contracts reject native storage. Missing state fails closed.

## Run

Use Rust 1.98 or later and the locked dependencies.

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

Inspect the JSON decision before authorizing execution. Examples without semantic evidence return `SEMANTIC_EVIDENCE_REQUIRED`.

For ordinary Rust type-checking:

```toml
allowit = { package = "allowit-sdk", path = "../AllowIt-sdk" }
```

The facade does not replace compilation or checked interpretation.

## Supported source and execution

The compiler validates every branch and rejects unregistered calls, mutation, loops, recursion, I/O and unsafe code. Source limits are 32 KiB/1,024 tokens.

Monetary guards require the bound six-decimal USDC asset and exact integer units. Adapters reject precision loss. The host independently enforces allocation. `set_cap_per_day` runs before control flow, using authenticated counters and UTC midnight resets.

Oracle input suspends for an authenticated, request-bound owner answer. Hosts enforce replay protection, expiry, revocation and fresh budgets. Contract input fails with `USER_INPUT_REQUIRED`.

## Preference questions and runtime JSON

`original_intent` and `runtime_context` each allow 16 KiB. Runtime JSON permits depth 8 and 128 entries. `context_u64` accepts only top-level non-negative integers.

Missing semantic evidence returns `SEMANTIC_EVIDENCE_REQUIRED`. The host asks Jev with original intent and complete context, then authenticates, persists and reevaluates. Bind evidence to policy/source/IR, owner, action, amount, recipient, network, asset, intent, context digest and expiry. Question hashes alone never authorize reuse. Point scores are not calibrated intervals. Deterministic restrictions always apply.

See the [agent skill](skills/allowit/SKILL.md) for context and host instructions.

## Embedding and wire protocol

Native callers use `process_value` or `process_json`. JSON evaluation compiles source and rejects supplied executable IR.

```json
{"operation":"compile","source":"pub async fn execute(ctx: &Context) -> PolicyResult { Ok(()) }"}
```

Compilation returns `policy`, hashes, workflow and IR. Evaluation accepts source, profile and context. Inspect `decision.outcome`, `code` and `reason`.

[execution requirements](docs/execution-requirements.md) identify dependencies without granting permissions.

- `edit_preference`: supply `source` and `settings: {step_id, auto_approve, approve_percent, auto_deny, deny_percent}`. Percentages are strings.
- `edit_score_thresholds`: supply `source`, `step_id` and all displayed `values` as integers from 0–10000. Only supported direct comparisons are editable.

Hosts authenticate edits. Stale IDs fail. Workflow spans use UTF-16 offsets. Diagnostic byte spans use UTF-8. Oracle `trace: true` records execution, not settlement. Bind traces to requests/revisions.

Enable `typed-workflow` for host workflows/providers. Typed APIs install authenticated records. Contracts authenticate IR without recompiling source. Use compatible registry 1.3 readers.

Provider policies bind an exact asset with `require_asset` or a monetary cap. Each policy supports one `paysh::call`. Typed adapters bind canonical input, settle the exact admitted amount, enforce payment/swap/fee ceilings and native limits, and deduplicate execution. Execute effects only from successful in-process evaluation. Contracts reject provider calls. Native policy-only evaluation requires verified owner/source/executor authority and actual funding.

## WebAssembly

```sh
rustup target add wasm32-unknown-unknown
cargo rustc --locked --release --lib --target wasm32-unknown-unknown --crate-type cdylib
```

Output: `target/wasm32-unknown-unknown/release/allowit_sdk.wasm`. Hosts enforce memory/time limits and discard instances after any trap.

- `alloc(len: u32) -> u32`: allocate input.
- `process(ptr: u32, len: u32) -> u64`: process UTF-8 JSON. High 32 bits hold the output pointer, low 32 bits its length.
- `dealloc(ptr: u32, len: u32)`: free each buffer once with its original length.

## Language server

`allowit lsp` provides diagnostics, hover, completion and `allowit/workflow` over stdio JSON-RPC. Workflow requests accept `textDocument.uri` or `source`.

## Verification boundaries

The [Lean demonstrator](verification/lean/README.md) models compositional decisions, not implementation equivalence or classifier accuracy.

## Readable amounts and comparisons

Use decimal string literals and retain `?`. Helpers require compiler revision `65a9bfe12f5abcbc7786a37b15f7f7a34f708dbd` or later. Invalid precision, syntax or overflow fails without rounding.

| Helper | Meaning |
| --- | --- |
| `usdc("25.50")?` | 25.50 USDC, exactly 25,500,000 units; up to six decimals. Zero is allowed in comparisons. |
| `percent("85.25")?` | 85.25%, exactly 8,525 basis points; 0–100 with up to two decimals. |
| `allowit::amount_at_most(ctx.amount_units, "25.50")?` | Whether this purchase is at or below 25.50 USDC, including equality. |
| `within_percentage_points(candidate, benchmark, "1")?` | Whether a candidate return is at most one percentage point below the benchmark. Both values are immutable integer variables or literals in basis points; 4% versus 5% passes. Higher returns pass. |

Use `set_cap_per_day` and `cap_per_transaction` for limits. Hosts authenticate compared values.

```rust
if !allowit::amount_at_most(ctx.amount_units, "25.50")? {
    allowit::require_user_input(ctx, "Approve this purchase above 25.50 USDC?").await?;
}
jev::check_preference(jev::preference_evidence(ctx, "Is there primary evidence supporting this purchase?"), "Is there primary evidence supporting this purchase?", 0.40, 0.85).await?;
```

### Preference thresholds

`local:dev` supports wallet-free oracle evaluation. Contracts reject it.

Thresholds are inclusive, exact decimals from 0–1 with at most four places. Denial must be below approval. `None` disables an outcome. `auto("deny")`/`auto("approve")` resolve to 0.40/0.85. Disabling both asks the owner directly. Missing or invalid evidence fails closed. Jev supplies `preference_fit` in [0,1]. Hosts round down to four places, retaining point-score semantics.

### Purchase tiers

`allowit::cap_purchase_tiers(ctx.amount_units, &ctx.token, &ctx.purchase_counts, 1_000_000, 2, "USDC", 6)?;` declares one unconditional top-level rule. Bands allow two purchases in (0.50,1], four in (0.25,0.50], then continued halving to the smallest unit.

Hosts supply 40 authoritative ledger counters, including pending reservations, and atomically recheck them. Runtime JSON cannot replace these counts. Default `oracle-ledger` builds support tiers. Contract builds reject tier IR with `LEDGER_REQUIRED`. Pin the SDK commit and artifact digest for compatibility.

## License

AllowIt-authored source is MIT licensed. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for third-party terms. Redistributed binaries require the root license and full [licenses/](licenses/) directory.
