# Execution requirements

Every compiler-produced `CompiledPolicy` includes `execution_requirements`, derived from its validated, lowered IR. This is a conservative inventory of supplemental evaluation dependencies. It is not a capability grant, an approval, a reachability analysis, or proof that the policy covers natural-language intent.

```json
{
  "version": 1,
  "features": ["owner_input", "runtime_context_u64", "semantic_evidence"],
  "context_u64_keys": ["candidate_yield_bps"],
  "dynamic_context_keys": false
}
```

The extractor visits conditions, both branches, call arguments, nested expressions, and statements after a return. Features and literal keys are deduplicated and emitted deterministically. A dependency can therefore appear even when a particular evaluation never reaches it. Consumers must not interpret the list as proof that a responder or provider exists, or that every listed capability will be used for the current request.

## Feature meanings

| Feature | Lowered operation | Required interpretation |
| --- | --- | --- |
| `semantic_evidence` | `semantic` | Bound semantic assessment evidence and original intent may be needed. The configured host supplies the Jev assessment; the SDK does not call a provider. |
| `confidence_evidence` | `confidence` | Named interval-shaped evidence may be needed. This does not establish statistical calibration or identify its provenance. |
| `owner_input` | `require_user_input` | An authenticated input continuation may be needed. Oracle evaluation can suspend; a reached input operation fails in the contract profile. It does not assert that an operator is available. |
| `purchase_history` | `cap_purchase_tiers` | Authoritative ledger counts, including applicable reservations, are required. The host owns ledger provenance and atomic updates. |
| `runtime_context_u64` | `context_u64` | Strict unsigned-integer request values may be read. Values remain claims unless their provenance establishes more. |

These are supplemental dependencies. An empty feature list does not remove the ordinary validated `Context`, spending/allocation state, exact request binding, authority or execution requirements. The inventory does not enumerate semantic questions, confidence-map slots, input prompts, supported rails or settlement capabilities. Those retain their existing source/IR and runtime protocols.

Source helpers contribute their lowered operations. A `check_preference` with both automatic outcomes disabled contributes `owner_input` only. A scored preference contributes `semantic_evidence` and `owner_input`, even when a particular score will decide without input. Pure decimal/comparison helpers add no external service dependency. Exact caller-label comparisons do not become semantic classification merely because this metadata is present.

## Context keys and integrity

`context_u64_keys` contains only string literals occurring directly as the key argument to `context_u64`. The extractor does not propagate constants: `let key = "yield"; context_u64(ctx, key)?` sets `dynamic_context_keys` to true and does not list `yield`. When that flag is true, the literal list is incomplete. Do not invent the missing keys or values; preserve the bound runtime context and resolve evaluation failures through the existing protocol.

Use metadata produced by compiling the exact source. Its association with `source_hash` and `ir_hash` comes from that compilation; the hashes do not independently hash this metadata object. SDK artifact validation recompiles the source and rejects metadata that differs from the recomputed requirements. Other consumers must likewise recompile or validate the association before trusting a persisted or externally supplied artifact. An unknown/missing requirements version or unknown feature needs explicit compatibility handling, not a default empty list.

## Extending the language

For each new IR primitive, declare its dependency mapping in `src/requirements.rs`, including an explicit no-additional-dependency mapping for pure operations. A validated call lacking that mapping produces `UNSUPPORTED_REQUIREMENT`; it must not silently disappear from the inventory. A source-only helper that lowers entirely to existing primitives inherits those primitives' dependencies.

Adding an expression/statement variant requires updating the exhaustive traversal. Adding new contextual state or changing an existing primitive's effects requires reviewing the mapping too; call-name exhaustiveness cannot discover that semantic change. Introduce a feature or schema version when the existing contract cannot express the dependency, and update consumers before claiming support.

Regression coverage should exercise nested expressions, both branches, post-return code, literal and dynamic keys, helper lowering, unknown primitives and metadata integrity. This supports the extractor's conservative contract for the current IR. A theorem that proves the implementation matches a formal semantics, and proof of full user-intent coverage, are separate obligations.
