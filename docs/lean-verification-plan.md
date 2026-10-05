# Lean verification after the frontend and vault updates

October 4, 2026. Historical proposed implementation plan; current checked model proofs and source-correspondence evidence are in [verification](../verification/README.md). This plan does not certify deployed code.

The owner subsequently assigned verification ownership to this task and specified literal native Rust execution with bounded daily-limit tuning. The authoritative [completeness contract](../verification/completeness.md) and [obligation ledger](../verification/obligations.json) cover that scope. This dated research plan's generic-template proposal is historical; current native-rule model/correspondence results are documented in [verification](../verification/README.md).

## Inspected revisions

These fetched branches are separate paths, not one integrated release:

| Repository / revision | Relevant behavior |
| --- | --- |
| AllowIt-app `61a2167`, customer workspace | Go `policy_template.go` compiles structured terms into restricted Rust. Budget, per-action limit, reserve, review amount, allowlists and expiry are explicit. This branch removes the typed assembler, preference editor and classification authoring guard, and pins an older SDK without their APIs. |
| AllowIt-app `b22cd60`, owner review | Descends from our `325a136`; retains the assembler and adds request review links and configured test mint handling. |
| AllowIt-sdk `2891518` | Validated IR is authoritative; `src/prelude.rs` is a type-checking facade. Thirteen Lean theorems cover a smaller model, without implementation correspondence. |
| SDK `2891518`, existing chain adapters | Solana/Stellar adapters execute IR in Contract profile with compiler/evidence authority and revision bindings. They are distinct from the fixed vault. |
| SDK `7a97502` / app `407596c`, paid vault | Separate, locally tested Devnet vault implements fixed limits, authorization and replay protection. It binds source/IR digests as provenance; it does not execute arbitrary IR. The payment journal distinguishes settlement from API delivery. |

The companion recommendation in the "Research Stellar and Solana Contract" task proposes shared Rust rules, Stellar executable references, and Solana custody plus versioned evaluators. Its inspected template is retained at `artifacts/allowit-igor-architecture-2026-10-04/igor-policy-template.go` in the outer workspace. It is a design dependency, not an implemented or approved release. Avoid merging the divergent frontend branches before reconciling their SDK pins, assembler and editor behavior.

## Specification and implementation boundary

Start with one pure, parameterized Rust rule module in the SDK. Give it typed configuration, explicit context provenance and a decision result. Reuse it in native app preview and chain evaluator builds; chain adapters authenticate inputs and perform custody operations. Keep the current custom-policy IR path until equivalent behavior is demonstrated. Importing generated source through today's prelude changes behavior, especially interactive approval.

Lean should specify this small rule module and the custody state machine first. Rust remains the production parser, AST editor, evaluator and chain implementation. Go remains transport, durable lifecycle and instruction assembly. Do not port the Go server to Lean or add three independent formal languages.

An executable Lean specification plus differential tests is a **tested correspondence**. A claim about all executions of Rust requires a proved implementation connection. Pilot [Aeneas](https://github.com/AeneasVerif/aeneas) on the extracted pure Rust module: check supported Rust constructs and generated semantics, then prove its relation to the Lean specification. Pin Aeneas, Charon and their required Lean version; do not assume compatibility with the demonstrator's Lean 4.11. Regenerate extracted definitions in CI and verify production call sites use the proved function. Inspect translation failures and external models explicitly. The frontend, storage, cryptography, compiler and chain runtime do not become verified through that pilot.

The pilot proves Aeneas's extracted model. Charon/Aeneas translation and rustc/LLVM/WASM/SBF compilation remain trusted components, not proved stages. Begin with the pure `vault.rs::check_payment` checks extracted away from Solana types; keep serde, dynamic collections, allocation and account APIs outside the first kernel.

[Cedar](https://docs.cedarpolicy.com/other/security.html) provides a useful precedent for separating a Lean specification, a Rust engine and differential testing. Its [symbolic compilation proofs](https://github.com/cedar-policy/cedar-spec/blob/main/cedar-lean/README.md) concern formal semantics; adopting that approach does not transfer Cedar's proofs to AllowIt.

## First proof obligations

| Boundary | Required property |
| --- | --- |
| Structured configuration | Valid decimal amounts map exactly to six-decimal integers; malformed or unsupported terms return an explicit error. Prove rule semantics for every valid configuration, including zero review/reserve settings and empty optional allowlists. |
| Decision kernel | Permission implies every applicable hard rule and required evidence check passed. Prove the reverse direction over an explicit valid domain and provide an allowed witness, so a deny-everything kernel cannot pass verification. |
| Contract state transition | Activation requires `0 < per_call ≤ allocation` and future expiry. Execution requires both vault/challenge expiries, exactly the next nonce and nonzero bindings. Success increases spending by exactly its positive amount, stays within limits, consumes the challenge once, and preserves asset, recipient and authority bindings. Rejection leaves state unchanged. Revocation is permanent; withdrawal cannot reopen spending. |
| Interactive approval | Input-required is not permission. A future contract permit must bind the owner, policy revision, exact action, amount, expiry and nonce; consuming it still rechecks hard limits. This permit path is new work. |
| Journal | Persist signed bytes before submission. Unknown outcomes retain their reservation and identity through restart. Reconciliation and delivery retry do not initiate a second payment. Payment finality and delivery success remain different facts. |
| Extensions / upgrades | A new rule supplies typed semantics, dependencies and preservation obligations. A new evaluator release must preserve custody invariants and explicitly state permitted behavioral changes. A version or "bug fix" label establishes no equivalence. |

Model chain atomicity and authenticated signatures as explicit adapter assumptions. Compiled SBF/Soroban tests must check those adapters against real account/CPI behavior. A Lean transition theorem alone does not prove deployed transfers, rollback or RPC finality. Prove delegation charging and bounded ancestry only when that feature is implemented; the fixed vault does not establish them.

For existing IR chain adapters, include the immutable envelope, monotonic active revisions and compiler/evidence attestations in the model. Compiler attestation remains the source-to-IR trust assumption. Contract-profile owner-input and purchase-tier rules must refuse rather than wait or accept a receipt bypass. The fixed vault records a request digest but does not authenticate its HTTP meaning; that remains an executor/gateway obligation.

If Solana splits custody from evaluation, do not give evaluator CPI custody signer authority or writable custody/token accounts. Custody checks mandatory limits and binds returned decisions to the request and approved evaluator revision before transferring. The current paid gateway requires a frozen program; patchable releases need an explicit deployment/authority attestation design.

Contract replay protection binds a vault and challenge, whereas journal idempotency binds a vault, stable request identifier and body hash. A new challenge can otherwise pay again for the same request. Proving request-level uniqueness requires durable journal retention, one namespace and all writers honoring the same PostgreSQL transaction advisory lock; file fixtures do not establish multi-instance durability. At `407596c`, a journal keeps existing records and rejects new identifiers once it reaches 200 requests; it does not evict records. Deletion or future pruning needs retained tombstones or an explicitly limited uniqueness claim.

Igor's reserve uses `allowit_available_cash_units`, supplied by the host from a finalized token balance minus same-network/mint reservations across policies. With an allocation source it observes one token account. Specify observation freshness, serialization and reservations. This is an observed balance, not an enforced chain-wide reserve against independent wallet spending. A controlled vault reserve can have a stronger invariant, but must not silently replace the existing meaning.

## Actual IR and skill coverage

Extend the reference language to match `src/types.rs`, `validation.rs` and `runtime.rs`: checked `u64` arithmetic, bindings, short circuiting, early returns, operation limits, global context validation and cap preflight before control flow. Preserve distinct failure, evidence-required and owner-input outcomes. Unsupported constructs yield `unknown`, never a proof claim.

Compare the Rust evaluator, requirement extractor and Go fragment selector against executable reference cases. Generate well-typed programs and adversarial boundary inputs; retain and minimize disagreements. Native, WASM and chain paths should replay the same supported cases. Every primitive must be included in the coverage inventory. Passing tests improve evidence; they are not universal refinement proofs.

Specify assembly as required features plus profile/interface dependencies, closed under fragment prerequisites. Prove conservative feature coverage and refusal when a requirement has no supported provider. Minimality concerns necessary fragment membership; unreachable branches and core protocol facts prevent claiming minimum word count.

Include service capability dependencies such as wallet transfer/owner-answer handling, and the current refusal of raw confidence evidence and unresolved dynamic context keys. Selection is relative to requirements, profile, capabilities and interface together.

Add provider/provenance metadata for context keys: caller claim, authenticated host observation or chain state. Igor's reserve key must not be requested as caller input. A requirement is not an available capability. Generated skills should state returned protocol results and supported transitions without assuming an operator, clickable UI, wallet, model service or executor is available. The owner-review branch's unconditional "show ... as a clickable link" needs this boundary reconciled. English interpretation and complete natural-language intent capture remain outside the theorem.

Classification of research purpose or customer-specific wallet roles remains a Jev assessment of the customer's predicate; matching the literal action `research` proves only identifier equality. Bind evidence to predicate/definition, policy revision and exact request/context. Missing, stale, malformed or mismatched evidence cannot permit. Prove threshold handling, not classification truth. Configurable decision thresholds and calibrated statistical confidence intervals are separate concepts; today's Noul point score supplies no calibration guarantee.

The SDK currently looks up semantic evidence by question hash; adapter checks must establish revision/request/context binding and authenticity of the configured Jev provider or evidence attester. Owner-answer keys likewise omit the request: prove the host stores and consumes an answer only for its immutable request, or change the key to include the request digest. Include replay across different requests at the same revision.

Pin `check_preference` semantics: enabled denial uses `upper ≤ deny`, enabled automatic approval uses `lower ≥ approve` after denial priority, otherwise the result remains unresolved/input-required as the profile permits. Both automatic outcomes disabled need no assessment. Include missing/invalid evidence and exact-boundary cases. Igor's generation prompt again suggests action-label classification and strict handwritten score comparisons; restoring only the UI does not restore these guarantees.

Before combining the frontend branches, restore typed requirements/assembly, the classification authoring guard and AST edit APIs against one pinned compiler artifact. Igor's freeform Instructions currently do not become enforced settings; adding Jev terms also requires integrating the template request path with semantic assessment. Proofs about numeric settings do not cover those instructions.

## Parameters, proof results and release checks

Current AST edits cover `check_preference` toggles/percentages and recognized raw semantic score bounds. Igor's monetary review threshold and other structured settings currently rebuild the template deterministically. Extend typed literal edits to budget/per-call/review/reserve/expiry values before claiming AST preservation for those settings. Never run the template printer over arbitrary custom policy source.

For semantic threshold edits, use the existing AST API and recompile deterministically. Instantiate universal parameter theorems after validating new ranges and bindings. No model regeneration or new hand-written proof is needed for an unchanged verified template. Editing must preserve unselected structure. A concrete policy equivalence or permission claim must be recomputed for its new parameters; old hashes cannot authorize the new revision.

An edit preservation theorem compares normalized structure with explicit source-location correspondence, not equality of complete old/new decisions. Existing owner-answer identifiers include the source binding, span and prompt; a new revision must invalidate old answers and any certificates even when only a literal changes.

For per-policy checks, export an independently reviewed, structured intent contract containing hard obligations, Jev predicates and explicit unknowns. Compare that contract with policy semantics, not with a copy of the same generated implementation. A proposed verifier returns `proved`, a replayable `counterexample`, or `unknown`; solver timeout/unsupported input is `unknown`. An SMT answer becomes a Lean-backed proof only through a sound verified decision procedure or checked certificate, otherwise the solver is an explicit trusted dependency.

Bind evidence to source/IR, configuration, registry, specification, toolchain, adapter/deployment and release revisions, with the assumptions and supported domain. Maintain separate fields for theorem checking, differential agreement and implementation refinement; do not collapse them into a "verified" badge. [Lean's validation guidance](https://lean-lang.org/doc/reference/latest/ValidatingProofs/) also distinguishes proving a statement from establishing that it means the intended claim. Audit axiom dependencies and independently recheck externally generated proofs before accepting certificates; the demonstrator's source scan is not a general hostile-proof verifier.

The first deliverable is the pure shared-rule kernel, its Lean semantics and conformance corpus, plus a feasibility result for Rust-to-Lean refinement. In parallel, model the existing fixed vault/journal transitions. Full IR coverage, source lowering, generated skill descriptors, upgrade refinement and a per-policy verifier follow as separate checked milestones. No public-chain or production security claim follows from the current thirteen proofs.
