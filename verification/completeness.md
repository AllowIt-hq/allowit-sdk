# Completeness contract

Verification owner: Codex in the policy/Lean task. This is the acceptance specification for assessing the other teams' work, not a claim that their current implementations satisfy it. The [ledger](obligations.json) records open obligations; the current ABI, adapter and deployment evidence must be attached before assessing native integration.

## What completeness means

Completeness is always relative to an explicit supported domain and independently stated requirements. Establish four connections:

1. **Intent to specification:** each owner requirement maps to a formal obligation or an explicit scope exclusion. Classification predicates preserve the customer's definition. Unresolved prose remains unresolved; translating it is not automatically a theorem.
2. **Specification to decision:** over well-formed inputs, permission holds exactly when the stated rules permit. Prove both directions, include successful witnesses, preserve specified error/continuation behavior and termination/resource limits. A deny-all implementation cannot satisfy this contract.
3. **Decision to effects:** authenticated adapters execute the permitted operation with the stated asset, amount, recipient, identity and atomic accounting. Evaluation alone has no custody authority.
4. **Effects to interfaces:** SDK, gateway, CLI, frontend and generated skills encode the same operation contract and report its outcome accurately. Every exposed operation is supported or explicitly refused.

Functional completeness says a supported operation succeeds when authorization, policy conditions, balance, fee and adapter preconditions hold and the operation is processed. It does not promise network inclusion, classifier truth, an available operator or eventual recovery. Those dependencies must be stated separately. Unsupported restrictions cannot be silently omitted from an accepted policy.

## Lean's role

Lean supplies executable definitions and universal proofs for the decision language and state transitions. Use the same expected vectors when testing native Rust, WASM, compiled SBF/Soroban adapters and applicable host/client paths. Verify the exact literal Rust policy used by both native contracts; a proof about the old IR evaluator does not establish native execution equivalence.

Pilot [Charon/Aeneas](https://github.com/AeneasVerif/aeneas) on the pure policy function. Pin the tools and required Lean backend, regenerate extraction and prove its relation to the independently reviewed specification. Its extracted model is the proved object; translation, Rust/LLVM/target compilation and external models remain explicit trusted components. Differential agreement is useful before refinement and remains a distinct kind of evidence. Account parsing, authorization, state storage and token calls require their own adapter obligations.

[Cedar's formalization](https://github.com/cedar-policy/cedar-spec/blob/main/cedar-lean/README.md) illustrates separate language, validation and symbolic-compiler proofs. AllowIt must prove its own semantics. [Lean proof validation](https://lean-lang.org/doc/reference/latest/ValidatingProofs/) requires checking the statement's meaning as well as the proof and dependencies. Review definitions and imported libraries, audit axioms, and independently recheck generated certificates against trusted statements. The current 4.11 demonstrator source scan is not a hostile-proof acceptance service; newer comparator/checker tooling requires its own pinned compatible environment.

## Native daily policy

The inspected native source defines `day = now / 86400`, with six-decimal policy units and `0 ≤ daily_limit ≤ 50_000_000`. These are current source facts, not universal product defaults. A changed bound or day definition requires updated evidence.

For `u64` inputs, set `effectiveSpent = spent` when `day == spent_day`, otherwise zero. The intended kernel statement is:

```text
evaluate(context) = Ok(next) iff
  approved
  and amount > 0
  and daily_limit <= 50_000_000
  and day >= spent_day
  and next = effectiveSpent + amount       // exact integer sum
  and next <= daily_limit
```

The bound on a successful result establishes that the addition fits `u64`; rejected overflowing inputs retain the explicit overflow outcome and its precedence. The current error order is not-approved, zero amount, parameter bounds, day regression, addition overflow, daily limit. Prove that order if callers rely on exact errors.

The counter belongs to a stable custody/accounting identity across revisions. Any later UTC day bucket (Unix-epoch chain seconds) grants one fresh allowance; skipped days do not accumulate allowances. This permits spending on both sides of midnight and is not a rolling 24-hour cap. Same-day timestamp regression is currently accepted; only regression to an earlier day is rejected. Time claims concern the authenticated chain timestamp, not exact offchain wall time.

Lowering the limit below already-spent is valid state. The invariant is **each accepted transfer fits the limit effective at that execution**, not `spent ≤ current_limit` after every update. Further same-day spending is blocked when no headroom remains. A zero limit pauses positive spending. Funding, tuning, approval/reapproval, withdrawal and upgrades must not manufacture a same-day reset. Only an authorized day transition may select fresh effective spending.

Offchain reservations assist scheduling; they do not authorize a transfer. The minimal execution adapter rechecks and transfers atomically using current chain state. If a reservation itself grants authority, its day, expiry, revision, amount, consumption and release rules become additional mandatory obligations. Midnight, a configuration change or an RPC timeout cannot resolve an uncertain prior payment.

The adapter must use checked signed-to-unsigned time conversion where required and persist the kernel-defined day and returned spending rather than diverging arithmetic. The pure `Context` lacks executor, asset, recipient, revision, method and replay identity. Those checks belong to adapters. Exact unit conversion is also mandatory: never equate raw chain units with policy units without verifying decimals and representability. Stellar classic asset amounts use seven decimals; six-decimal policy amounts require checked conversion where applicable.

## Native handoff identities and transitions

The integration plan at AllowIt-app `3a0ae62bd6b454217d75bf666fd83e5c6eb9ef22`, `docs/solana-mvp-integration-plan.md`, supplies the following design boundaries. The ledger records its exact bytes as a design input. These are obligations to check against the delivered ABI and implementation, not new proof or deployment evidence.

- Keep portable Rust source identity separate from each chain's compiled/deployed artifact identity. Custody identity is also separate from policy executable identity. The Solana adapter must verify the reviewed loader/ProgramData association, immutable policy code and deployed identity; a source hash alone establishes none of these.
- Separate editor/server save revision, on-chain state/parameter revision and standing module approval. Owner-authorized in-bounds tuning preserves standing approval and spending counters, advances the state revision and makes old unsigned requests stale. Verify the compiled tuning ceiling independently in custody state or the verified artifact; a module's reported bound is insufficient.
- Signed requests made uncertain before tuning keep their exact identity and proof. A new state revision cannot establish that the earlier transfer failed or authorize a replacement signature/payment.
- An in-place module switch withdraws standing approval and preserves counters. Its contract tests remain required even though the MVP UI uses a new reviewed activation for enforcement-code replacement. Retire the old standing execution authority before authorizing the replacement; retain funds, counter history and unresolved recovery identities. A new vault has a separate daily allowance, which must be disclosed rather than presented as a preserved global wallet cap.
- Host comparisons, compiled-VM tests, connected local-RPC execution and finalized public Devnet acceptance are distinct evidence domains. Report their network, target and exact artifacts separately.

V03, V06, V10, V11, V15 and V23 cover these boundaries. The current Lean daily kernel accepts an already-supplied approval boolean and state; it does not prove these transition or authentication properties. Additional Lean state/refinement work follows implementation readiness and does not gate completion of the immediate Solana integration plan.

## Required coverage

The stable IDs below are detailed in the ledger. A semantic or paid-API extension assessment must select a supported native base profile and include its obligations recursively, then add its extension obligations. An extension cannot be assessed alone or inherit an unmet proof. Explicit ABI-backed exclusions remain visible.

| IDs | Required property and evidence |
| --- | --- |
| V01–V03 | Independent intent/domain inventory; exhaustive method, primitive, outcome and state-effect inventories; same literal native source and exact source/build/compiled-artifact/deployment bindings. |
| V04–V05 | Kernel permission equivalence, error order, nonvacuity and exact numeric conversion. Include zero/max/overflow inputs, each failed predicate and every inclusive boundary. |
| V06–V09 | Authenticated context and approval; exact authorized transfer; daily counter transition; deposit independence. VM tests inspect actual token effects and rollback. |
| V10–V13 | Bounded tuning, upgrade continuity, replay/concurrent spending and method closure. Preserve spending/day/replay across configuration changes; malformed input and unsupported methods cannot become permissive transfers. |
| V14–V16 | Preview/read operations have no protected effects; durable submit/retry/reconcile retains identity and signed proof; every interface faithfully maps outcomes and uncertainty. |
| V17–V19 | Frontend fields map to exact contract semantics; generated descriptors/skills cover supported dependencies without assuming caller capabilities; AST edits preserve unrelated structure and reject stale/unsupported edits. |
| V20–V22 | Semantic profiles bind Jev predicates/evidence and configurable threshold handling; calibration remains separate; paid-API profiles distinguish payment from delivery and never recharge on delivery retry. |
| V23 | Proof acceptance and dependency-aware invalidation bind formal statements, assumptions, implementation connection and fresh release evidence. |
| V24 | If withdrawal is exposed, prove declared owner authority, exact custody effects, explicit daily-limit applicability and no approval/accounting/replay reset; otherwise record an ABI-backed exclusion. |

The frontend's current total budget, per-action cap, monetary review threshold, reserve, allowlists and expiry are not aliases for a native daily limit. Each needs supported native semantics or explicit refusal/retention in a separate profile. Existing owner-review UI may present a request, but the new ABI must define what authorization it conveys. Source authoring and active daily-limit tuning are distinct operations.

Classifications such as research purpose or customer-specific wallet roles require Jev assessment of the customer's predicate. Exact action/merchant/recipient identifiers remain deterministic scope facts. Missing, stale, malformed or mismatched assessment cannot authorize a semantic restriction. Pin inclusive approval/denial boundaries and denial priority. Noul's point score is not a calibrated confidence interval; empirical calibration is a separate obligation and assumption set.

Current semantic AST edits require no model regeneration. They produce a newly bound source artifact and invalidate old approvals/certificates. They cannot become additional mutable on-chain fields through the daily-tuning method. For an unchanged parameterized theorem, instantiate it with valid new parameter data; recheck instance-specific claims and all changed bindings.

## Implementation handoff

Each team submits one evidence packet bound to exact revisions:

- **Contracts:** exhaustive ABI/state/event/error inventory; signer and upgrade model; policy/API source hashes; dependency/toolchain/build locks; artifacts and deployment identity; before/request/after vectors; compiled VM results including failed-transfer rollback and real asset effects.
- **SDK:** typed operation/configuration descriptors, strict encodings, decimal conversion, supported profiles, edit preservation contract and vectors. Native and legacy IR paths remain distinct.
- **Gateway:** durable transitions, request identity/retention, signing and broadcast order, context providers, reservation rules and concurrent/crash traces. State how every writer obtains serialization.
- **CLI/frontend/skills:** every action/command/control/result mapped to an ABI operation or explicit refusal; actual wire captures; authentic status projections; absent-capability handling; generated skills for each supported profile. `show`, `eval` and `status` cannot spend; `exec` cannot infer finality from submission.
- **Semantic/provider extensions:** customer predicate/definition versions, evidence bindings and attester authority; threshold vectors; unavailable-provider behavior; calibration provenance if claimed. Paid delivery additionally supplies request-bound payment/result receipts and retry rules.

Metadata and hashes establish identity. They do not prove semantics or authenticity by themselves. The verification owner validates evidence, rather than treating a submitter's `verified: true` label as a result.

## Independent acceptance corpus

Maintain expected outcomes from this specification, independent of implementation branches. For every supported operation include a successful witness, each refusal boundary and a state-effect frame check. Cover exact day boundaries, skipped/backward days, zero/max limits, lowering below spent, deposits after spending, stale revisions, wrong signer/asset/recipient, failed token calls, duplicate/uncertain submission and configuration changes during pending operations.

Replay applicable vectors through all real paths; mock transfer plans establish only mock behavior. Property-based generation and minimized differential disagreements supplement universal proofs. The Python reference is a third implementation of this specification, not independent evidence that natural-language intent was captured. Lean compilation/runtime and host rustc/runtime are trusted for replay; kernel checking establishes the model theorems separately. Mutation checks should detect removal of the daily cap, a reset on deposit/tuning, a wrong decimal conversion, an unsupported-method fallback and treating submitted/unknown as executed. A test suite that survives such mutations is incomplete for those obligations.

Operation coverage is derived from the delivered dispatch/ABI and client inventories, not a manually selected happy-path list. New methods, primitives, fields, providers and outcomes add obligations. Syntax-derived dependency coverage is conservative and does not imply permission, reachability or an available executor.

## Assessment and change handling

For each obligation report independently: specification status, checked model theorem, implementation connection, adapter/client execution evidence, deployed-artifact identity, assumptions and unresolved counterexamples. Delivery completion, a checked model and universal production refinement are different claims. A concrete per-policy query returns `proved`, replayable `counterexample` or `unknown`; unsupported constructs/timeouts cannot count as proved. SMT results require a checked certificate or a verified decision procedure for a Lean-backed claim, otherwise list the solver as trusted.

Identity checks and harness tests use their own evidence levels; they are not behavioral correspondence. Ledger entries supply receipt hashes, review identity, date and host metadata, and tests compare them with the source lock, model and receipt dependencies. Evidence must identify the requirement/domain, source/configuration/parameter revisions, formal statement and definitions, tool versions, proof/test logs, target/artifact and reviewer. Do not compute an overall assurance percentage from theorem/test counts.

Invalidate affected evidence when policy/API source, parameter bounds, units, primitive registry, context provider, adapter, authorization, migration, compiler/translation, ABI, client projection or deployment changes. Universal template proofs may survive in-range tuning; request approvals, instance claims and parameter/deployment bindings must still be refreshed. Preserve old evidence as a historical result rather than silently retargeting it.

Current status: 13 earlier bounded-model theorems and 16 native daily-rule theorems checked. The [native receipt](native-correspondence.json) records host Rust/Lean/specification correspondence, kernel mutation detection and exact source/tool dependencies. This is finite tested correspondence, not universal Rust refinement. Adapter/ABI/build/deployment and client completeness remain open. This assessment does not block a separately scoped Solana demonstration on completion of the entire formal program.
