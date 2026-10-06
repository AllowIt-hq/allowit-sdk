# Lean proof demonstrator

`AllowIt.lean` is an executable, machine-checked model of feature selection and a small policy decision language. It is **not a proof of the Rust SDK, Go assembler or generated Markdown**. The production-to-model connection has not been proved or differentially tested.

## Run

Use the pinned stable toolchain in `lean-toolchain`:

```sh
cd verification/lean
elan toolchain install leanprover/lean4:v4.11.0
python3 check.py
```

With an independently installed official release, `LEAN_BIN=/absolute/path/to/lean python3 verification/lean/check.py` works from the repository root. The script rejects a missing or different checker, rejects proof holes/custom axioms/native proof evaluation, checks the file, and audits every theorem's dependencies. Lean's standard `propext` and `Quot.sound` axioms occur; no application-specific axiom is assumed. The executable example's `#eval` output is a demonstration, separate from its kernel-checked `example_passes` theorem.

CI can run the same two commands after installing Elan; it must fail if the pinned checker cannot run. There are no Mathlib or other package dependencies. Version 4.11.0 was chosen to keep the standalone checker small; the model does not require newer Lean features. [Official release](https://github.com/leanprover/lean4/releases/tag/v4.11.0).

## Proved domains

| Model | Universal property |
| --- | --- |
| `Flow` built from feature uses, sequence and binary branches | Every feature encountered by any evaluated branch belongs to the conservative all-branch requirement list. |
| `selected` | Selected feature membership equals required feature membership; every candidate covering the requirements contains the selection. This is minimal feature membership, not minimum prose or reachable-path analysis. |
| Capability availability | A required feature absent from available capabilities makes the flow unsupported. No capability or actor is assumed to exist. |
| `assess` | Passing requires present, ordered evidence within 0–10,000 basis points, enabled approval at/above its threshold, and no enabled denial at/below its threshold. Missing evidence never passes; both automatic outcomes disabled gives `unresolved` even without evidence. |
| Finite `Policy` trees | Ordered evaluation passes exactly when the declarative permission specification holds. Both directions are proved, so denying everything does not satisfy the permission specification. |
| Every budget leaf in a passing policy | The positive requested amount plus committed spending fits both allocation and cap, without unsigned 64-bit overflow. Classification cannot override a budget leaf. |
| `examplePolicy` | A concrete budget-plus-classification request passes, establishing a nonempty approval example. |

Features use the SDK's five identifiers: `confidence_evidence`, `owner_input`, `purchase_history`, `runtime_context_u64`, `semantic_evidence`. Adding a feature requires updating the typed enumeration and selection universe; the exact-coverage theorem then checks that the selection includes it. `Flow` models conservative traversal, including inactive branches. It has no early returns or expression short-circuiting and makes no claim about a reachable approval path.

`Policy` models permit, a budget gate, a supplied classification assessment and ordered conjunction. Amounts are `Fin (2^64)` with exact natural-number arithmetic; intervals are `Fin 10001` with a proof of ordered endpoints. `committed` represents a trusted snapshot including relevant reservations, not a proved concurrent ledger. Malformed external input and stale bindings are outside these typed inputs.

This policy model omits source parsing, general SDK expressions, `set_cap` preflight before control flow, resource limits, owner-input continuation, network/asset binding, purchase tiers and settlement. `permit` can pass without a budget, unlike the SDK's global context checks. The model therefore proves neither SDK evaluation equivalence nor a production authorization invariant. `unresolved` does not assert that an operator exists or that a harness can obtain an answer.

Jev's output is an external observation. No theorem establishes classification truth, personalization correctness, calibration, natural-language intent completeness or execution capability. A Noul point score does not establish a calibrated interval. The interval model specifies how valid bounds would be handled; it does not supply them.

## Architecture implication

The [October 4 verification plan](https://github.com/ackrate/ackrate-project/blob/main/instance/artifacts/107-allowit-repository-artifacts/AllowIt-sdk/docs/lean-verification-plan.md) incorporates the divergent customer workspace, owner-review flow and separate fixed-policy Devnet vault. It prioritizes shared-rule semantics and custody/journal transitions, then the actual IR bridge; all production proof milestones remain proposed.

Lean can own the typed language specification, executable reference evaluator, extension laws and proofs. Rust can continue to own parsing, source-preserving edits, validated IR, deterministic execution and WASM/contract adapters. Go can own transport, lifecycle and capability-aware skill assembly. A verified Lean model plus Rust/Go tests remains a tested correspondence; a production correctness claim needs a proved refinement or a verified/extracted implementation with its trust boundary stated.

A useful next bridge is to compare the actual Rust requirement extractor and Go fragment selector with this model over generated typed trees, including every registered function and both branches. That would add differential evidence, not a theorem about either implementation. A separate lowering theorem should establish that `check_preference`'s actual IR implements the interval/disabled-state specification. A skill renderer can then derive fragments from the verified machine descriptor; its English interpretation still is not a formal proof.

The closest primary reference is Cedar: [its Lean language and compiler proofs](https://github.com/cedar-policy/cedar-spec/blob/main/cedar-lean/README.md) and [symbolic-compilation theorems](https://cedar-policy.github.io/cedar-spec/docs/Cedar/Thm/SymbolicCompilation.html). Cedar distinguishes these proofs from [differential testing of its Rust validator](https://docs.cedarpolicy.com/policies/validation.html). Lean explains [kernel checking](https://lean-lang.org/doc/reference/latest/Elaboration-and-Compilation/) and [axiom auditing](https://lean-lang.org/doc/reference/latest/Axioms/).
