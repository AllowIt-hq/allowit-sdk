<div align="center">

# Allowit SDK

*Write the rules your AI agent spends by, in a few lines of Rust.*

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)
[![Rust 1.98+](https://img.shields.io/badge/Rust-1.98%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![no_std + alloc](https://img.shields.io/badge/no__std-%2B%20alloc-lightgrey?style=flat-square)](#embed-it)

**Go on. On your terms.**

[Overview](#overview) • [Quick start](#quick-start) • [Write a policy](#write-a-policy) • [CLI](#the-allowit-cli) • [Embed it](#embed-it) • [Repository map](#repository-map) • [The Allowit system](#how-this-fits-the-allowit-system)

</div>

Allowit lets your AI agents spend and invest, within a policy you set. This repository is where those policies are born: the restricted Rust dialect they are written in, the compiler that checks them, the deterministic interpreter that decides them, and the tools that make writing them pleasant.

## Overview

An Allowit policy is a small Rust function. It reads a request (`ctx`), runs a sequence of guards such as "no more than 100 USDC per day" or "only this merchant", optionally asks the owner a question, and either passes or fails. Nothing else is possible, by construction.

```rust
use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap_per_day(ctx, "100", "USDC")?;
    cap_per_transaction(ctx, "10", "USDC")?;
    require_merchant(ctx, "research.example")?;
    check_preference(
        ctx,
        "Does this purchase count as research under the user's stated purpose and definitions?",
        true,
        "85",
        true,
        "40",
    )
    .await?;
    Ok(())
}
```

That is [`examples/research.rs`](examples/research.rs), and it compiles as written. The SDK turns it into a validated intermediate representation (IR) with content hashes, a human-readable workflow, and a list of what the policy needs at runtime. Hosts evaluate the IR; they never execute arbitrary code.

### Features

- **A language owners can read.** Policies are plain Rust with decimal amounts like `"25.50"`, so `usdc("25.50")?` means exactly 25.50 USDC, with no floating-point rounding.
- **A compiler that says no.** Every call must come from the function registry. No I/O, no `unsafe`, no unbounded loops, no surprises.
- **One interpreter everywhere.** The same `no_std + alloc` core runs in the CLI, in WebAssembly, and inside the on-chain adapters, so a decision is the same wherever it is made.
- **Two evaluation profiles.** `oracle` can pause to ask the owner a question; `contract` fails closed with `USER_INPUT_REQUIRED` because a smart contract cannot wait for a human.
- **Deterministic money.** Daily caps, per-transaction caps and purchase tiers use exact integer units and host-authenticated counters, and the daily cap runs before the policy's own control flow.
- **Preference questions with thresholds.** `check_preference` turns a natural-language question into an auditable step with explicit approve and deny thresholds.
- **Language server included.** `allowit lsp` speaks JSON-RPC over stdio with diagnostics, hover, completion and a workflow view.
- **Chain-aware output.** `compile` reports which Stellar wallet implementation fits the source, and `stellar-native` emits the generated Rust for a custom one.

## Quick start

You need [Rust](https://rustup.rs) 1.98 or later. Then:

```sh
git clone https://github.com/allowit-hq/allowit-sdk.git
cd allowit-sdk

# Compile a policy and inspect its IR, hashes and workflow
cargo run --locked -- compile examples/research.rs

# Evaluate it against a request, as the hosted oracle would
cargo run --locked -- evaluate examples/research.rs examples/context.json oracle
```

The second command prints a decision (abridged below; the full output also carries the evidence key, source span and hashes). For this example it is `SEMANTIC_EVIDENCE_REQUIRED`: the policy asks a preference question, and nobody has assessed it yet. That is the SDK doing its job. A host obtains an assessment bound to this request, re-evaluates, and only then can the request pass.

```json
{
  "decision": {
    "code": "SEMANTIC_EVIDENCE_REQUIRED",
    "outcome": "fail",
    "question": "Does this purchase count as research under the user's stated purpose and definitions?",
    "reason": "A preference assessment is required for this request."
  },
  "ok": true
}
```

> [!TIP]
> `ok: true` means the operation ran. The verdict lives in `decision.outcome` and `decision.code`. Always read the decision before letting anything move.

More to try:

```sh
cargo run --locked -- workflow examples/approval.rs                                   # owner-facing step list
cargo run --locked -- evaluate examples/approval.rs examples/context.json contract    # same policy, contract profile
cargo run --locked -- evaluate examples/green-investments.rs examples/green-context.json oracle
cargo run --locked -- registry                                                        # every supported function, with help text
```

## Write a policy

### Shape

A policy has one entrypoint and reads everything through `ctx`. Two forms are accepted:

- `pub async fn evaluate(ctx: &Context) -> PolicyResult` with `use allowit::prelude::*;`, as in the [examples](examples/).
- `async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult` with `use allowit::v1::prelude::*;`, a `PolicyParams` struct and a private `new()` constructor, as in the [policy template](templates/policy/). Constructor values are immutable configuration; changing them means a new approved artifact.

Guards are sequential. Each `?` stops evaluation with a reason the owner can read.

The [policy language reference](docs/policy-reference.md) covers the details this page skips: source limits, owner-adjustable native limits, Stellar engine selection, workflow edits and purchase tiers.

### Money

Amounts are decimal strings and the SDK keeps them exact.

| Write | Meaning |
| --- | --- |
| `usdc("25.50")?` | 25.50 USDC, exactly 25,500,000 base units; up to six decimals |
| `percent("85.25")?` | 85.25 %, exactly 8,525 basis points; 0 to 100 with up to two decimals |
| `amount_at_most(ctx, "25.50")?` | Is this request at or below 25.50 USDC? |
| `within_percentage_points(candidate, benchmark, "1")?` | Is the candidate return at most one point below the benchmark? |

`set_cap_per_day` and `cap_per_transaction` are the limits most policies start with. The daily cap resets at UTC midnight and uses counters the host authenticates; request JSON cannot talk its way around them.

### Questions

Some rules are not arithmetic. `check_preference` asks one question in plain language, and the host brings back evidence bound to this exact request. Its thresholds are explicit and inclusive: in the example above, `true, "85"` approves at a score of 85% or more, `true, "40"` denies at 40% or less, and anything in between comes back to the owner. Pass `false` to switch either outcome off. `require_user_input(ctx, "Approve this purchase?")` skips the scoring and asks the owner directly.

### Runtime context

Evaluation takes a JSON request such as [`examples/context.json`](examples/context.json): amount and allocation in base units, action, merchant, recipient, token, network, the current time, the owner's original intent, and a `runtime_context` object the policy can read with `context_u64`. The [agent skill](skills/allowit/SKILL.md) walks through assembling one.

### A template to start from

```sh
cargo test --locked --manifest-path templates/policy/Cargo.toml
cargo run --locked -- compile templates/policy/src/policy.rs
```

[`templates/policy`](templates/policy/) is an ordinary Cargo crate whose `src/policy.rs` you replace with your own source. It type-checks with `rustc` and compiles through the real policy compiler, so your editor and the SDK agree.

## The `allowit` CLI

This repository's binary is the developer tool for policy source. (The agent-facing `allowit` command that sends requests to a running Allowit service lives in [allowit-cli](https://github.com/allowit-hq/allowit-cli); use `cargo run --locked --` here to keep the two apart.)

```
allowit compile|check|workflow POLICY.rs
allowit evaluate POLICY.rs CONTEXT.json [oracle|contract]
allowit stellar-native POLICY.rs
allowit registry
allowit json [REQUEST.json|-]
allowit lsp
```

Pass `-` to read a policy or a JSON request from stdin. The JSON commands print pretty output and exit non-zero when `ok` is false.

| Command | What you get |
| --- | --- |
| `compile`, `check`, `workflow` | The compiled policy: validated IR, `source_hash`, `ir_hash`, execution requirements, the owner-facing `workflow` step list, and the recommended Stellar engines |
| `evaluate` | A decision for one request under the `oracle` (default) or `contract` profile |
| `registry` | Every supported function with title, signature and description |
| `json` | Any wire-protocol request, for example `edit_preference` or `compile_stellar_wallet` |
| `stellar-native` | Generated Rust for a custom Stellar policy wallet, or a clear refusal |
| `lsp` | A stdio language server with diagnostics, hover, completion and `allowit/workflow` |

## Embed it

The crate is the wire protocol. Native callers use `allowit_sdk::process_value` or `process_json` with the same request objects the CLI builds:

```json
{"operation":"compile","source":"use allowit::prelude::*; pub async fn evaluate(ctx: &Context) -> PolicyResult { Ok(()) }"}
```

Cargo features select how much of the SDK you take along:

| Feature | Purpose |
| --- | --- |
| `std`, `compiler`, `oracle-ledger` | The defaults: full compiler, CLI and purchase-tier ledger support |
| `typed-workflow` | Typed workflow records for hosts and services (pulled in by `compiler`) |
| `compact-contract-validation` | Error codes instead of diagnostics for `no_std` contracts |
| *(none)* | The bare `no_std + alloc` interpreter that the contract adapters use |

For WebAssembly hosts:

```sh
rustup target add wasm32-unknown-unknown
cargo rustc --locked --release --lib --target wasm32-unknown-unknown --crate-type cdylib
```

The module at `target/wasm32-unknown-unknown/release/allowit_sdk.wasm` exports `alloc(len)`, `process(ptr, len)` and `dealloc(ptr, len)`; `process` takes UTF-8 JSON and packs the output pointer and length into one `u64`. Hosts set memory and time limits and discard an instance after any trap.

To type-check policy source in an ordinary Rust project, depend on the facade:

```toml
allowit = { package = "allowit-sdk", path = "../allowit-sdk" }
```

The facade is for your editor. Compilation and evaluation still go through the SDK.

## Repository map

| Path | Crate or package | What it is |
| --- | --- | --- |
| [`src/`](src/) | `allowit-sdk` | Compiler, interpreter, registry, workflow projection, protocol, CLI, LSP |
| [`examples/`](examples/) | | Three policies and two request contexts used throughout this README |
| [`templates/policy/`](templates/policy/) | `allowit-policy-template` | A Cargo crate to write and type-check your own policy in |
| [`client/`](client/) | `allowit-client` | The client facade that [allowit-cli](https://github.com/allowit-hq/allowit-cli) is built on: agent keys, wallet setup, owner approvals, request signing |
| [`client-core/`](client-core/) | `allowit-client-core` | Shared configuration, errors, journals and private storage for clients |
| [`services/`](services/) | `allowit-services` | Typed facade over service vendors and rail protocols |
| [`contracts/`](contracts/) | `allowit-contract-core`, `allowit-solana` | On-chain adapters that evaluate compiled IR and move tokens in the same transaction |
| [`stellar-rust/`](stellar-rust/) | `allowit-stellar-wallet` | Stellar wallet preparation, dual-party authorization and settlement |
| [`native-rust/`](native-rust/) | `allowit-native` | Native Solana policy lifecycle in Rust |
| [`native/`](native/) | `@allowit/native` | The same lifecycle for Node 22+, with the owner browser handoff |
| [`tempo-rust/`](tempo-rust/) | `allowit-tempo` | Compatibility re-export of the Tempo vendor crate |
| [`docs/`](docs/) | | Policy language reference, execution requirements, native vault and Tempo vault notes |
| [`skills/allowit/`](skills/allowit/SKILL.md) | | Instructions for an AI agent that compiles and evaluates policies |
| [`verification/`](verification/) | | Lean specifications, obligations and expected outcomes |
| [`scripts/`](scripts/) | | WebAssembly smoke test and license packaging |

## How this fits the Allowit system

Allowit is a small constellation of repositories, and this one sits in the middle.

- **This SDK** defines the policy language and decides requests. Everything else trusts its hashes and its IR.
- [**allowit-cli**](https://github.com/allowit-hq/allowit-cli) is the command an agent actually runs: `show`, `eval`, `exec` and `status` against a running Allowit service, built on this repository's `client/` facade.
- [**allowit-contracts-solana**](https://github.com/allowit-hq/allowit-contracts-solana), [**allowit-contracts-stellar**](https://github.com/allowit-hq/allowit-contracts-stellar) and [**allowit-contracts-tempo**](https://github.com/allowit-hq/allowit-contracts-tempo) hold the on-chain custody that enforces an approved policy on each rail. `contracts/`, `stellar-rust/` and `tempo-rust/` here are the adapters that speak to them.
- The hosted Allowit service and app, introduced at [allowit.xyz](https://allowit.xyz), own HTTP transport, owner input and the durable record of each request. Restricted Rust stays the source of truth for what an agent may do.

A compiled artifact is bound by its `source_hash` and `ir_hash`, so an owner can read the exact source that a wallet enforces, and a contract can verify that it is running the policy the owner approved.

## Development

```sh
cargo fmt --all --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo check --locked --no-default-features --lib     # the no_std core still builds
```

CI pins Rust 1.98.0 and also exercises the individual feature flags, the WebAssembly build (`node scripts/test-wasm.mjs`), and each sub-crate under its own manifest. The workflows in [`.github/workflows/`](.github/workflows/) are the authoritative list; [`AGENTS.md`](AGENTS.md) explains how to pick the right ones for a change.

## License

Allowit-authored code is released under the [MIT License](LICENSE). Dependencies keep their own terms; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the [`licenses/`](licenses/) directory, and ship both with any redistributed binary.
