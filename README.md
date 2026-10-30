# AllowIt SDK

AllowIt policies use a restricted subset of Rust. This repository contains the compiler, checked interpreter, registry, workflow projection and language server.

```rust
use allowit::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::spent_cap(grant::spent_cap)?;
    allowit::day_total_cap(grant::day_total_cap)?;
    allowit::recipients(grant::recipients)?;
    if ctx.amount_units == 0 {
        return Err(PolicyError {
            code: "ZERO_AMOUNT".into(),
            reason: "A positive amount is required.".into(),
        });
    }
    Ok(())
}
```

The Go [action CLI](https://github.com/AllowIt-hq/allowit-cli) provides customer `show`, `eval`, `exec` and `status` commands. The Rust executable here provides developer compiler commands.

## Policy rules and placement

The eight basic rules are `allowit::spent_cap`, `allowit::day_spent_cap`, `allowit::day_total_cap`, `allowit::transaction_cap`, `allowit::call_cap`, `allowit::recipients`, `allowit::expiry` and `allowit::tokens`. Each takes one value or Grant reference. Other helpers use their exact registered namespace and Rust signature. Ordinary Rust `Err(PolicyError { ... })` returns a refusal.

Standard Stellar wallets enforce the consecutive initial calls that use the same-named Grant reference, as shown above. Any subset is valid. Order and duplicates remain explicit. Extraction stops at the first other statement. Conditional calls, computed values, literals and later calls run offchain. The server executes the complete `_execute` source. Its failure discards all working accounting changes. The source profile records each rule's placement.

Missing or Null Grant values skip the selected rule. Nonnull values must have the correct type. Numeric caps accept zero. `recipients` and `tokens` use native address lists. A deposit's destination is its vault. Token checks use the actual asset address.

`spent_cap` limits cumulative irreversible expenses. `day_spent_cap` applies the same measure per UTC day. Ordinary payment principal counts as spent. Deposit principal and temporary redeemable loss remain separate. `day_total_cap` counts gross debit plus separately attributable gas. It does not add embedded conversion expenses twice.

Stellar amounts use the installed SAC's seven-decimal base units. Vault shares keep their independent scale. `allowit::usdc` always returns six-decimal units. Its result is not an implicit Stellar amount conversion. Other profiles retain their authenticated denomination. Use `u64` suffixes or typed variables for large literal rule inputs.

`evaluate_with_policy_rules` and `evaluate_with_trace_policy_rules` require host-only operation facts and authoritative accounting. Public Context JSON cannot install those capabilities. Native adapters retain their separate funding and authorization checks. Unavailable expense valuation fails when a nonnull expense rule executes. Unpriced historical expenses prevent the corresponding active cap.

The standard wallet binds the ordered prefix, native module, asset and schemas. Its digest excludes mutable Grant values and the complete offchain source. Owner Grant updates preserve protected history. Offline source updates bind the complete source hash, logical source instance, wallet, network, prefix and next revision. An unchanged prefix needs no chain transaction. Clients retain their approved bundle until an explicit owner-approved refresh.

`compile_stellar_wallet` requires an explicit engine and rail. Simple handles an empty prefix. Common preserves the selected prefix. The developer Custom generator emits that prefix and identifies the offchain remainder. It does not compile arbitrary custom conditions into native code. `require_onchain` rejects a selection whose complete source needs offchain evaluation.

Named-value helpers return local copies or require an explicit native binding. A canonical accounting context does not authorize persistent custom state writes. The current single-context source does not provide the former functional State argument. Those helpers do not imply that a custom suffix can replace kernel history.

The [policy template](templates/policy/README.md) checks the source with rustc and the restricted compiler. The [Stellar adapter guide](stellar-rust/README.md) covers native requests, signatures and receipt accounting.

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

Inspect the JSON decision before authorizing execution. Missing native facts or semantic evidence prevent approval.

For ordinary Rust type-checking:

```toml
allowit = { package = "allowit-sdk", path = "../AllowIt-sdk" }
```

The facade does not replace compilation or checked interpretation.

## Supported source and execution

The compiler validates every branch and rejects unregistered calls, mutation, loops, recursion, I/O and unsafe code. Source limits are 32 KiB/1,024 tokens.

Adapters authenticate denomination, funding and exact integer units. The complete offchain policy follows its actual control flow. Only the explicit initial Grant prefix receives standard native placement.

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

Enable `typed-workflow` for host workflows/services. Typed APIs install authenticated records. Contracts authenticate IR without recompiling source. Registry 1.8 identifies the canonical one-argument rules. New constructs cannot claim an older registry. Retained signed history uses its original bytes and binding.

Service policies declare one exact actual asset with an unconditional `allowit::tokens(&["actual-asset-id"])?;` call. The host must install that authenticated asset identity. Each policy supports one `allowit::service_call`. Typed adapters bind canonical input, settle the exact admitted amount, enforce payment/swap/fee ceilings and native limits, and deduplicate execution. Execute effects only from successful in-process evaluation. Contracts reject service calls. Native policy-only evaluation requires verified owner/source/executor authority and actual funding.

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
| `allowit::usdc("25.50")?` | 25.50 USDC, exactly 25,500,000 units; up to six decimals. Zero is allowed in comparisons. |
| `allowit::percent("85.25")?` | 85.25%, exactly 8,525 basis points; 0–100 with up to two decimals. |
| `allowit::within_percentage_points(candidate, benchmark, "1")?` | Whether a candidate return is at most one percentage point below the benchmark. Both values are immutable integer variables or literals in basis points; 4% versus 5% passes. Higher returns pass. |

Use the canonical rules for spending caps. Plain comparisons can express additional source conditions. Hosts authenticate compared values.

```rust
if ctx.amount_units > allowit::usdc("25.50")? {
    allowit::require_user_input(ctx, "Approve this purchase above 25.50 USDC?").await?;
}
allowit::check_preference(allowit::preference_evidence(ctx, "Is there primary evidence supporting this purchase?"), "Is there primary evidence supporting this purchase?", 0.40, 0.85).await?;
```

### Preference thresholds

`local:dev` supports wallet-free oracle evaluation. Contracts reject it.

Thresholds are inclusive, exact decimals from 0–1 with at most four places. Denial must be below approval. `None` disables an outcome. `auto("deny")`/`auto("approve")` resolve to 0.40/0.85. Disabling both asks the owner directly. Missing or invalid evidence fails closed. Jev supplies `preference_fit` in [0,1]. Hosts round down to four places, retaining point-score semantics.

### Purchase tiers

`allowit::cap_purchase_tiers(ctx.amount_units, &ctx.token, &ctx.purchase_counts, 1_000_000, 2, "USDC", 6)?;` declares one unconditional top-level rule. Bands allow two purchases in (0.50,1], four in (0.25,0.50], then continued halving to the smallest unit.

Hosts supply 40 authoritative ledger counters, including pending reservations, and atomically recheck them. Runtime JSON cannot replace these counts. Default `oracle-ledger` builds support tiers. Contract builds reject tier IR with `LEDGER_REQUIRED`. Pin the SDK commit and artifact digest for compatibility.

## License

AllowIt-authored source is MIT licensed. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for third-party terms. Redistributed binaries require the root license and full [licenses/](licenses/) directory.
