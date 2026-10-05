# Retained transaction frame review

The independent review used Claude Code explicitly selecting `claude-opus-5-5`; the completed JSON model-usage field verifies that exact model. Tools were disabled. This is a supplied-text model/code/scope review: no reviewer inspected production Rust, authenticated observations or reproduced the runtime or Lean checks. The producer owns those checks. Full 292-entry store literals were omitted from the closure packet; generator, theorem definitions/inventory, logs, lock and receipt were supplied.

The [initial report](2026-10-05-transaction-frames-initial.json) required repairs before acceptance. Its session is `01300d3a-b658-444b-9e40-1f463c4581e7`.

| Finding | Disposition |
| --- | --- |
| Aborted request absent from new Lean certificate | Decode both transfers independently, bind full actions/program/custody and emit identical-action/state-nonce/revision certificates. |
| Failure-site tags had no formal meaning | Remove them from the formal fee relation. Locked Python validators separately check observed outcomes and nested logs. |
| Numeric payer arithmetic disconnected from store | Remove seven closed arithmetic-only certificates; keep the exact whole-store fee relation and distinguish three derived nonce corollaries. |
| Raw bytes disconnected in new generator | Check metadata/hash equality, duplicate keys, custody discriminator/padding/owner and transfer/token/mint/authority/ProgramData bindings. |
| Fixed readiness=True in successes | Quantify over any readiness predicate with an explicit environment hypothesis. |
| Parser/source/dependency hardening | Refuse duplicate JSON keys; cross-bind old bindings/observations and current manifest/source; screen explicit snapshot imports; pin Python identity; sanitize Git environment; retain only relevant Solana source pins. Standard-library/transitive execution stays trusted. |
| Fixture scope | State LiteSVM fee profile, enforce System payer/nonalias/no System instructions. No fee collector or rent-exemption claim. |
| Toy-only negative controls | Add actual retained-store digest perturbation and captured failed-request nonce perturbation, both rejected by Lean, alongside three generic controls. |

Producer validation: 141 Python tests pass. Fresh isolated Lean source compilation checks the unchanged NativeDaily/State imports and new Frame/Corpus files, with complete theorem inventory and only `propext`/`Quot.sound` axioms. Ten generic statements, 16 finite certificates and three derived nonce corollaries are checked. A second fresh default acceptance reproduced receipt `1bdc8734a275265095591bd33b958ef9abfe13927ac7ea0de8024a653c6250ab` exactly. Historical corpora/models/receipts remain unchanged. No VM executes in this layer.

All 24 release obligations remain open. Account enumeration, observed-byte authenticity, hashing/decoding, runtime/cryptography/Clock fidelity and official cached-library build fidelity remain trusted. This is not source refinement, universal adapter/rollback/readiness proof, compiled build provenance, client or deployment validation.

The [closure report](2026-10-05-transaction-frames-closure.json), session `90864275-92f7-4a52-bbdb-a0594d5c6b16`, approved partial V07/V08/V12/V23 evidence with no new blocking issue. It confirmed all four earlier material findings resolved. Its non-blocking follow-ups were addressed by requiring isolated Python/no bytecode caching, allowing only the exact historical NativeDaily evaluation, documenting shared-checkout staleness, adding negative mutation coverage for every listed binding path, and targeting a single protected account in the corpus digest control. The optional stronger payer lemma was not added: exact fee effect already follows from the complete store equality; the generic arithmetic lemma remains accurately scoped.

The [final report](2026-10-05-transaction-frames-final.json), session `22b80f2e-fc8f-49f8-a057-affe77a9e0de`, accepted the hardening with no new material bug, conditional on second default receipt acceptance; the producer verified that byte equality. The final wording clarifies interpreter-specific freshness, bytecode write/read distinctions, and separates the historical evaluation from identity rechecks. These are documentation clarifications only.

Remaining non-blocking findings are retained: corpus negative templates lack their own paired positives (the generic fee template has one); command screening does not cover every execution construct such as `#guard`; the exact historical evaluation line allowlist is protected principally by its frozen whole-file hash, rather than a parsed command boundary; isolation flags are checked after imports and guard misuse rather than providing a hostile-input sandbox. The optional stronger payer lemma remains future work. No soundness claim is made for the parser, acceptance screen or negative-control causality. These do not invalidate the independently checked finite theorem statements under the declared trusted producer/input boundary.

After wording-only README clarifications and whitespace cleanup, a fresh source/tool/library check regenerated delivery receipt `269e2bc211bc28d12ebd6df8e9f3f3b7ddd7d72eba0e4d4cff8c3ed8ff2b44d7`; proof logs and formal statements remained byte-identical to the reviewed candidate.
