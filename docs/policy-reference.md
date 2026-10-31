# Policy language reference

The [README](../README.md) gets you writing policies. This page collects the rules you will want once you go further: what the compiler accepts, how owner-adjustable limits work, how a Stellar wallet is chosen, and which wire-protocol requests edit a policy.

## What the compiler accepts

- A policy is at most 32 KiB of source and 1,024 syntax tokens.
- Every branch is validated. Unregistered calls, mutable variables, recursion, I/O and `unsafe` are rejected. Loops are limited to bounded loops over typed collections.
- Monetary guards require the bound six-decimal USDC asset and exact integer units. Adapters reject precision loss, and the host enforces the allocation independently.
- Amount helpers such as `usdc("25.50")?` and `percent("85.25")?` fail on invalid precision, syntax or overflow. They never round.

## Constructor configuration

In the `allowit::v1::prelude` form, `new()` returns the policy's immutable configuration. It accepts inline `u64`, `bool`, `&'static str` and bounded `&'static [&'static str]` literals. Changing a value means a new approved artifact.

### Owner-adjustable native limits

Native policies can declare two limits that the owner may later adjust on chain:

```rust
use allowit::v1::prelude::*;

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

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    let limit = allowit::stored_limit(ctx, params.action_limit)?;
    if ctx.amount_units > limit {
        return allowit::fail("Over the per-purchase limit");
    }
    Ok(())
}
```

`allowit::stored_limit` reads the adapter-verified `Context.native_policy_storage`. Only these two keys exist. JSON evaluation and portable contracts reject native storage, and missing state fails closed.

### Named state on Stellar

A functional Stellar policy receives the request, the owner's `Grant` and the current `State` separately, and returns the exact spent map to persist. `grant_get` and `state_get` read a typed `u64`, Boolean or string with a source default. `state_set`, `state_remove` and `with_spent` return changed copies, and only the final returned map persists. A denial or failed native effect rolls the operation back. The [CLI README](../cli/README.md) shows a complete example.

## Stellar engine selection

`compile` responses include `stellar_engines`: the smallest fixed Stellar wallet implementation that preserves the whole policy.

| Engine | What it does |
| --- | --- |
| `SimplePolicyEngine` | Custody plus agent and service authorization; the service enforces the policy |
| `CommonPolicyEngine` | Executes an owner-selected ordered list of canonical rules on chain |
| `CustomPolicy` | Compiles bespoke control flow and local rule mixins |

Common rejects discarded rule results, branching, repeated rules and custom map changes; those policies need Custom. Classic G-address wallets support Simple-style signing with service-enforced scalar policies. They pay classic reserves and no Soroban custody rent, and they cannot run Common or Custom rules. Stateful native policies need Common or Custom. Source that needs external evidence has no Stellar recommendation.

The `compile_stellar_wallet` request takes `source`, `network` (`stellar:testnet` or `stellar:mainnet`), `rail` (`classic` or `policy_wallet`), `engine` and `require_onchain`. It returns the selected implementation, the exact Common rules, the source digest and, for Custom only, generated Rust. The SDK never substitutes weaker enforcement when a selection fails. From the CLI, `allowit stellar-native POLICY.rs` emits Custom policy source.

## Preference questions

`check_preference(ctx, question, auto_approve, approve_percent, auto_deny, deny_percent)` takes thresholds as exact percentage strings from 0 to 100 with up to two decimals. Both are inclusive, and denial must be below approval. Disabling both outcomes asks the owner directly. Questions are 1 to 1,024 bytes.

Missing evidence returns `SEMANTIC_EVIDENCE_REQUIRED`. The host obtains a `preference_fit` score, authenticates and stores it, then re-evaluates. Evidence is bound to the policy, source, IR, owner, action, amount, recipient, network, asset, intent, context digest and expiry; a question hash alone never authorizes reuse. Scores are point estimates, not calibrated intervals, and every deterministic rule still applies.

Runtime limits: `original_intent` and `runtime_context` each allow 16 KiB, and `runtime_context` allows depth 8 and 128 entries. `context_u64` reads only top-level non-negative integers of `runtime_context`.

The `oracle` profile suspends for an authenticated, request-bound owner answer; hosts enforce replay protection, expiry, revocation and fresh budgets. The `contract` profile returns `USER_INPUT_REQUIRED`. The `local:dev` network supports wallet-free oracle evaluation, and contracts reject it.

## Purchase tiers

```rust
cap_purchase_tiers(ctx, "1", 2, "USDC")?;
```

This declares one unconditional top-level rule. The first band allows two purchases above 0.50 and up to 1 USDC; each cheaper band halves the ceiling and doubles the count, down to the smallest unit. Hosts supply authoritative ledger counters, including pending reservations, and recheck them atomically; runtime JSON cannot replace them. The default `oracle-ledger` feature supports tiers, and contract builds reject tier IR with `LEDGER_REQUIRED`.

## Editing a compiled workflow

Two wire-protocol requests let a host change thresholds without the owner touching source:

- `edit_preference` takes `source` and `settings: {step_id, auto_approve, approve_percent, auto_deny, deny_percent}`. Percentages are strings.
- `edit_score_thresholds` takes `source`, `step_id` and every displayed value in `values`, as integers from 0 to 10000. Only supported direct comparisons are editable.

Hosts authenticate edits, and stale step IDs fail. Workflow spans use UTF-16 offsets; diagnostic byte spans use UTF-8. An `evaluate` request with `trace: true` records execution, not settlement.

## Registry versions

The current registry is `1.7.0`, which adds named state and functional native policies. Programs written for registry `1.6.0` stay valid, but new constructs cannot claim the older registry. Unchanged source keeps its source and IR hashes when its lowering is unchanged.
