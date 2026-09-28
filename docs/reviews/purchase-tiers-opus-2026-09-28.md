# Purchase tiers and compact source review

Reviewer: Claude Code, actual model `claude-opus-5-5` in both responses. Scope: SDK evaluator/compiler and shared contract adapters; host ledger implementations were not shared or reviewed.

The initial early-return finding was fixed by hoisting purchase tiers before control flow. JSON and binary contract validation reject tier artifacts, and the floating point source facade is excluded from no_std builds. Native SDK tests, clippy, no_std, WASM compilation and common contract tests passed locally.

# Follow-up review: purchase-tier bypass fix, contract guard, facade gating

**Verdict: no material blockers in the shared SDK and contract diff.** I reviewed only the diff and files included in your message, without tools. I did not review host integration (Go and engine ledger counts, reservations, locking, `Context` construction). Those remain your stated responsibility.

## Finding 1 (HIGH bypass): resolved

The fix works because several pieces combine:

- **The validator pins the call's shape.** `cap_purchase_tiers` must appear exactly once, as a top-level `Stmt::Expr { Try(Call) }`. Any other shape fails to compile, whether nested, conditional, `let`-bound, or repeated. `tier_count` is incremented before the `config` check, so nested copies also count toward the limit.
- **The hoist matches that exact shape.** `src/runtime.rs:529` hoists this pattern. Because the validator guarantees the shape, the single call is always hoisted and runs before `evaluator.block`. Early `return`, conditional returns and earlier `require_user_input` all come later, and your regression test covers all three for both profiles.
- **Running the check twice is harmless.** The call executes again at its original position, but `check_tiers` is a pure check.
- **The Contract profile is covered three ways:**
  - The hoisted call returns `LEDGER_REQUIRED` under `Profile::Contract`.
  - `prepare_execution` sets `purchase_counts: None`, which also yields `LEDGER_REQUIRED`.
  - `validate_chain_program` now runs on both the JSON path (`validate_decoded_artifact`) and the binary path, so the artifact is rejected before evaluation.

## Non-blocking items

1. **Wider scope of `validate_chain_program`.** It now runs inside `validate_decoded_artifact`, so every caller of `validate_artifact` gets the full chain profile, including the depth limits. Before release, confirm two things:
   - Existing activated mandates all pass it. They should if activation always used `validate_chain_artifact`. Otherwise they now fail closed at `prepare_execution`.
   - No oracle-side host loads artifacts through `allowit_contract_core::validate_artifact`. If one does, purchase-tier policies (and deeper oracle-only IR) become unusable there.

   Both failure modes fail closed, so this is an availability risk, not a bypass.
2. **Zero maximum.** The validator message says "positive maximum", but only the upper bound is checked. If `amount_units("0")` returns `0`, then `cap_purchase_tiers(ctx, "0", n, "USDC")` compiles into a rule that rejects every purchase with `PURCHASE_CAP_EXCEEDED`. That fails closed, but you should either reject zero or fix the message.
3. **Missing test for the binary path.** The conformance test checks tier rejection through `validate_chain_artifact` and the JSON `prepare_execution` path. It does not check `validate_binary_chain_artifact` or `validate_binary_artifact`. The code path is there; the test isn't.
4. **Direct indexing.** `values[2]` in the runtime tier arm is still indexed directly. It is safe because `run()` validates first, but `.get(2)` would add defence in depth.
5. **`std` feature for contract crates.** The facade gating is correct only if every contract crate depends on `allowit_sdk` with `default-features = false`, or with a feature set that excludes `std`. You say the no_std and WASM builds pass. I would still add the wasm float-opcode CI check as a cheap guard against a future default-feature change.
6. **Carried over and accepted as source-compatibility only:**
   - Import and arity are still not tied together.
   - The registry shows only the 4-arg `check_preference` signature.
   - The error text for a wrong arity still describes only the 6-arg form, and the signature error still names only `evaluate`.
   - `REGISTRY_VERSION` is unchanged even though an opcode was added. Old evaluators fail closed on the unknown function. Compact preferences lower to the existing 6-arg IR, so they stay compatible.

## Checked and found sound

- **Compact lowering:** both threshold slots map correctly, and `auto` is tied to its matching direction.
- **Integer thresholds:** the `Lit::Int` path parses them exactly, and the facade's `From<i32>` impl lets them type-check.
- **Import parsing:** `allowit::v1::prelude::*` is parsed consistently in `valid_import` and `validate_signature`. `v2`, `other::v1` and doubled `v1` are rejected.
- **Bands and shifts:** band ≤ 39 and `first_count << band` < 2^59, so there is no out-of-bounds index or overflow. The ledger length check is exactly 40.
