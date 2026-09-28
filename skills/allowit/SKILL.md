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
if candidate + 100 < benchmark {
    return fail("The expected return difference exceeds one percentage point");
}
```

This differs from a relative 1% decrease; preserve the owner's intended unit in the policy. No preference score can override this numeric failure, a spending cap, a wrong recipient or a wrong network.

An oracle `awaiting_input` result requires authenticated owner approval through the engine's continuation protocol. Never reuse an answer from another request. A smart contract fails every reached `require_user_input` call even if an answer exists. Cancellation, expiry, replay protection and fresh budget checks belong to the engine and rail.

Runnable policies and contexts are in `examples/research.rs`, `examples/approval.rs`, `examples/green-investments.rs` and `examples/green-context.json`. `allowit lsp` provides editor diagnostics, function help and the `allowit/workflow` projection from the same compiler.
