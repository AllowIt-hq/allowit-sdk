# AllowIt verification

This space owns Lean specifications, completeness obligations, independent expected outcomes and assurance reports. Contract, SDK, gateway, CLI and frontend agents own their implementations. The verification owner reviews those implementations against independently stated requirements and records remaining gaps; implementation tests alone do not define the expected behavior.

Start with [completeness](completeness.md). [The obligation ledger](obligations.json) is the machine-readable inventory. [Lean](lean/README.md) contains executable specifications and checked proofs. The [October 4 research plan](../docs/lean-verification-plan.md) is background; the native-source and daily-limit requirements below supersede its generic-template assumptions.

## Current scope

The immediate integration demonstration is native Solana policy execution through the frontend, Go gateway, SDK and CLI. The contract effort also targets the same literal Rust policy source on Stellar. Deposits are separate from policy approval/revisions. Only the daily-limit parameter is tunable within bounds declared in Rust; tuning, funding and upgrades must preserve accounting. Unimplemented methods fail explicitly. Payment-provider/MPP delivery and broader semantic execution are extensions, not prerequisites for this demonstration.

The currently inspected native source is `policy/policy.rs` in the two contract repositories. Its pure decision function checks approval, positive amount, a daily limit bounded by 50,000,000 six-decimal units, the chain-time day bucket and checked addition. Source identity does not establish adapter or compiled-code correctness. Native contract ABIs and local build manifests are now committed; their compiled behavior, deployments and connected clients still require validation. Native execution completeness remains unassessed.

## Ownership and handoff

The verification owner maintains requirement IDs and domains, formal statements, source/model mappings, expected vectors, counterexamples, proof acceptance and evidence freshness. Each implementation team supplies its operation/schema inventory, exact source/build revisions and reproducible execution traces. [Required evidence](completeness.md#implementation-handoff) specifies the packet.

Delivery validation and formal assurance are separate milestones. This space does not add a full-refinement gate to the immediate demonstration. It reports which claims have model proofs, tested correspondence, implementation proofs or only external assumptions. A demonstration can pass while formal refinement remains open; it must not be described as complete formal verification.

## Evidence rules

- Specify the intended claim before reviewing its proof. Include permitted cases and a successful witness as well as forbidden cases.
- Bind every result to its domain, configuration, code, specification and toolchain. Recheck affected evidence after changes.
- A valid Lean proof establishes its formal statement. A production claim additionally needs an implementation connection and explicit adapter/compilation assumptions.
- Unknown, unsupported, missing or stale evidence remains visible. A count of passing tests or theorems is not a completeness score.
- Classifier correctness, natural-language interpretation and network availability are not silently assumed. Semantic restrictions use customer-bound Jev assessment and configured decision thresholds; statistical interval claims require separate calibration evidence.

The existing 13 theorems establish only their bounded model. The separate native model has 16 checked theorems; [source-bound host replay](native-correspondence.json) reports finite correspondence and mutation detection. The [ledger](obligations.json) records that partial evidence while keeping production obligations open. Future universal statements about compiled contracts remain conditional on the identified compilation and chain-runtime trust boundaries.

The [9:13 p.m. readiness check](readiness/2026-10-04-2113.json) is retained as a historical snapshot of the uncommitted candidates. It does not describe the subsequent committed contract handoff.

The [published contract handoff](published-handoff.md) now has committed targets and a separate source-bundle/artifact intake checker. Its 2,907-vector host receipt preserves the earlier frozen evidence. Source inspection finds the announced artifact and ceiling hardening; adapter/client/deployment and universal source refinement remain open.
