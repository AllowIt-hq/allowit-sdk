# AllowIt verification

This space owns Lean specifications, completeness obligations, independent expected outcomes and assurance reports. Contract, SDK, gateway, CLI and frontend agents own their implementations. The verification owner reviews those implementations against independently stated requirements and records remaining gaps; implementation tests alone do not define the expected behavior.

Start with [completeness](completeness.md). [The obligation ledger](obligations.json) is the machine-readable inventory. [Lean](lean/README.md) contains executable specifications and checked proofs. The [October 4 research plan](../docs/lean-verification-plan.md) is background; the native-source and daily-limit requirements below supersede its generic-template assumptions.

## Current scope

The immediate integration demonstration is native Solana policy execution through the frontend, Go gateway, SDK and CLI. The contract effort also targets the same literal Rust policy source on Stellar. Deposits are separate from policy approval/revisions. Only the daily-limit parameter is tunable within bounds declared in Rust; tuning, funding and upgrades must preserve accounting. Unimplemented methods fail explicitly. Payment-provider/MPP delivery and broader semantic execution are extensions, not prerequisites for this demonstration.

The currently inspected native source is `policy/policy.rs` in the two contract repositories. Its pure decision function checks approval, positive amount, a daily limit bounded by 50,000,000 six-decimal units, the chain-time day bucket and checked addition. Source identity does not establish adapter or compiled-code correctness. Native contract ABIs and local build manifests are now committed; their compiled behavior, deployments and connected clients still require validation. Native execution completeness remains unassessed.

## Ownership and handoff

The [extracted kernel refinement](refinement/README.md) now checks the unchanged committed Rust kernel through pinned Charon/Aeneas and a separate Lean 4.31 environment. Its universal equality covers ordered errors, exact success, termination and every bounded input; success also agrees with the independent permission predicate. This is conditional on translator/compiler fidelity and trusted upstream library artifacts. Compiled adapters, clients and deployments remain outside the proof. The Lean 4.11 model and earlier receipts are preserved.

The verification owner maintains requirement IDs and domains, formal statements, source/model mappings, expected vectors, counterexamples, proof acceptance and evidence freshness. Each implementation team supplies its operation/schema inventory, exact source/build revisions and reproducible execution traces. [Required evidence](completeness.md#implementation-handoff) specifies the packet.

Delivery validation and formal assurance are separate milestones. This space does not add a full-refinement gate to the immediate demonstration. It reports which claims have model proofs, tested correspondence, implementation proofs or only external assumptions. A demonstration can pass while formal refinement remains open; it must not be described as complete formal verification.

The separate [adapter state specification](adapter/README.md) proves custody maintenance and replay requirements, with pinned ABI/source inventories and a separate replay of existing compiled Solana tests. Its handwritten model is not extracted custody code, and the cached VM executable has no checked build provenance. All obligations remain open.

The separate [custody trace correspondence](traces/README.md) links a fresh isolated Rust/Mollusk harness to the independent specification through 32 finite Lean certificates. It covers 26 ordinary instruction executions and one explicitly invalid-supply diagnostic. Readiness, real signature authentication, universal adapter refinement, program build provenance, clients and deployment remain unproved; all 24 obligations remain open.

The separate [finite model refusal certificates](refusal/README.md) prove that none of the 12 captured non-environment program-rejected requests admits a successful model transition, for any readiness predicate or post-state. A positive budget-case witness keeps environment failure separate. The prior trace corpus, models and receipts remain unchanged; this does not establish universal adapter refinement.

The separate [single-fault transfer witnesses](isolation/README.md) leave spending headroom after a same-module rebind and isolate five modeled refusal conditions. Twelve observed VM executions generate 28 checked statements, including two generic completeness links and five positive model counterfactuals. This does not discharge platform readiness or prove an executable repair/authority grant.

## Evidence rules

- Specify the intended claim before reviewing its proof. Include permitted cases and a successful witness as well as forbidden cases.
- Bind every result to its domain, configuration, code, specification and toolchain. Recheck affected evidence after changes.
- A valid Lean proof establishes its formal statement. A production claim additionally needs an implementation connection and explicit adapter/compilation assumptions.
- Unknown, unsupported, missing or stale evidence remains visible. A count of passing tests or theorems is not a completeness score.
- Classifier correctness, natural-language interpretation and network availability are not silently assumed. Semantic restrictions use customer-bound Jev assessment and configured decision thresholds; statistical interval claims require separate calibration evidence.

The existing 13 theorems establish only their bounded model. The separate native model has 16 checked theorems; [source-bound host replay](native-correspondence.json) reports finite correspondence and mutation detection. The [ledger](obligations.json) records that partial evidence while keeping production obligations open. Future universal statements about compiled contracts remain conditional on the identified compilation and chain-runtime trust boundaries.

The [9:13 p.m. readiness check](readiness/2026-10-04-2113.json) is retained as a historical snapshot of the uncommitted candidates. It does not describe the subsequent committed contract handoff.

The [published contract handoff](published-handoff.md) now has committed targets and a separate source-bundle/artifact intake checker. Its 2,907-vector host receipt preserves the earlier frozen evidence. Source inspection finds the announced artifact and ceiling hardening; adapter/client/deployment and universal source refinement remain open.

[Transaction rollback preparation](runtime/README.md) specifies a separate signed-transaction/store experiment. It records pinned upstream source research only; no new runtime execution or release obligation is claimed.

The separate [LiteSVM loader preflight](runtime/loader.md) built the pinned upstream and observed both unchanged published SBF files loading, accessor-byte equality and invalid-ELF refusal. It executes no instruction/transaction and adds no rollback/readiness or dependency-closed refinement claim.

The separate [signed transaction/store experiment](runtime/transaction/README.md) records seven submissions, a committing multi-transfer control, a transfer-prefix/unsupported-instruction abort and nonce reuse, plus signature/blockhash negatives. Independent strict Python wire/account checks support finite LiteSVM store correspondence, with no Lean transaction theorem or fully closed build provenance.

The separate [custody failure boundaries](runtime/failure-boundary/README.md) experiment records token-CPI budget failure and token success followed by failure of that same custody instruction, with persistent-store frames and successful nonce reuse. It supplies finite trusted-runtime evidence, no universal refinement or deployment proof.
