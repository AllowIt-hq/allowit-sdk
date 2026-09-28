---
name: allowit
description: Compile, inspect and evaluate AllowIt Rust policies with structured runtime JSON, including preference questions and deterministic spending limits.
---

# AllowIt policies

Use the installed `allowit` CLI or this repository's `cargo run --locked --` commands. Compile the exact policy source before evaluating it. A valid compilation does not approve a transaction.

1. Read the original owner instructions and the immutable policy source. Preserve the original instructions across revisions and forks; never replace them with your own summary.
2. Put the complete available request context in a JSON file. Supply `amount_units`, `allocation_units`, `spent_units`, `action`, `merchant`, `recipient`, `token`, `network`, `now`, `original_intent` and `runtime_context`. Amounts use exact integer micro-USDC. Runtime context is an object, at most 16 KiB/depth8/128 entries. Original intent is at most 16 KiB. Include source/provenance information for claims.
3. Run `allowit compile POLICY.rs` and inspect its exact source, limits and workflow. Use `allowit registry` for supported functions and their help.
4. Run `allowit evaluate POLICY.rs CONTEXT.json oracle` or submit the equivalent JSON request to the authenticated engine. Inspect `decision.outcome` and `decision.code`; CLI process success alone is not approval.
5. A `pass` permits only the exact bound request within the host's authenticated mandate. It is not evidence that funds moved. The executing rail must enforce that mandate again and return an actual transaction result.

## Runtime JSON

```json
{
  "amount_units": 10000000,
  "allocation_units": 250000000,
  "spent_units": 0,
  "action": "investment",
  "merchant": "issuer.example",
  "recipient": "the exact destination address",
  "token": "USDC",
  "network": "devnet",
  "now": 1790590000,
  "original_intent": "Prioritize green investments and avoid hype without sacrificing more than one percentage point of expected annual return.",
  "runtime_context": {
    "candidate_yield_bps": 420,
    "benchmark_yield_bps": 500,
    "candidate_name": "Clean-energy bond",
    "environmental_claims": ["Proceeds fund renewable generation"],
    "sources": [{"url": "https://issuer.example/disclosures", "kind": "issuer disclosure"}]
  },
  "answers": {},
  "confidence": {}
}
```

Caller-provided facts are claims until authenticated by the host. Do not invent balances, evidence, answers or scores. The CLI accepts context for local evaluation; the production engine must reconstruct authoritative wallet, budget, timestamp and mandate fields itself and verify evidence provenance.

## Preferences and numeric rules

`semantic(ctx, "exact question")?` requests an assessment of a preference. Missing evidence fails with `SEMANTIC_EVIDENCE_REQUIRED`, `question` and `evidence_key`; the key is lowercase SHA-256 of the exact UTF-8 question. The trusted engine may ask Jev by TypeSafe with the question, original owner instructions and complete bound runtime context, authenticate and persist the response, then reevaluate. This is a host operation; the policy and SDK make no network calls. Never turn that missing-evidence failure into a pass yourself or send context to an arbitrary provider.

The question hash names a slot within one evaluation, not reusable approval. The trusted engine must bind evidence to the exact source/IR digests, revision, owner, action, amount, recipient, network, token, original intent, complete runtime-context digest and expiry. Never copy a score to another request or retrieve it by question hash alone. Changed context requires fresh applicable evidence. If the authorized provider path is unavailable, report the unresolved failure to the owner and stop that transaction.

If Jev supplies a point score, using equal `lower_bps` and `upper_bps` records a **point score**, not a statistically calibrated confidence interval. A calibrated interval needs separate supporting provenance. Missing, malformed or unverifiable evidence fails closed. Host-supplied `confidence` and `answers` are privileged; ordinary agents must not populate them to bypass checks.

Keep hard constraints deterministic. For a maximum loss of **one percentage point (100 basis points)**, `context_u64` reads strict integer values and this expression enforces the rule:

```rust
let candidate = context_u64(ctx, "candidate_yield_bps")?;
let benchmark = context_u64(ctx, "benchmark_yield_bps")?;
if !within_percentage_points(candidate, benchmark, "1")? {
    return fail("The expected return difference exceeds one percentage point");
}
```

This differs from a relative 1% decrease; preserve the owner's intended unit in the policy. No preference score can override this numeric failure, a spending cap, a wrong recipient or a wrong network.

An oracle `awaiting_input` result requires authenticated owner approval through the engine's continuation protocol. Never reuse an answer from another request. A smart contract fails every reached `require_user_input` call even if an answer exists. Cancellation, expiry, replay protection and fresh budget checks belong to the engine and rail.

Runnable policies and contexts are in `examples/research.rs`, `examples/approval.rs`, `examples/green-investments.rs` and `examples/green-context.json`. `allowit lsp` provides editor diagnostics, function help and the `allowit/workflow` projection from the same compiler.

## Readable amounts and comparisons

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

## Preference gates and Local dev

Use `check_preference(ctx, "Exact preference question", true, "85", true, "40").await?;` for a configurable Jev gate. The flags enable automatic approval at or above 85% and denial at or below 40%. Other scores require the owner's answer. Both flags may be disabled; that asks the owner without calling Jev. Thresholds are exact decimal percentage strings from 0 to 100 with at most two decimals; denial must be below approval when both outcomes are enabled. A preference pass cannot override other constraints. The host's explicit assessment result is an object with exactly one numeric field, `preference_fit`, from 0 to 1; it is not calibrated confidence or multiple estimated dimensions. Context is provided as JSON into the CLI or SDK and forwarded by the trusted host with the exact question and original owner intent.

`local:dev` runs only in the oracle profile. Local records consume the local budget without a wallet or blockchain settlement. A consumer-generated SKILL.md can contain a private scoped access URL and bearer header. Treat that file as a credential, send it only to the intended agent, keep authorization on its specified origin, and obey its expiry/revocation. The capability does not authorize independent wallet spending.
