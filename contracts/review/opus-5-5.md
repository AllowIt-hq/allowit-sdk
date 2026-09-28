# Contract source review — 2026-09-28

Independent reviewer: Claude Code, explicitly requested and completed model `claude-opus-5-5` (verified from `modelUsage`). Review session: `68031d6f-8b3c-4799-9a0d-740947310371`.

Candidate manifest SHA-256: `f9247c9d34117e1f8fb02d422b5ccffdad4b79e5eff29457c3d89d0d3be3f0c7`. The [file manifest](reviewed-files.sha256) identifies the exact 23 source, test, lock and workflow files reviewed; this report itself was added afterward. The user explicitly approved sharing these private adapters and shared evaluator for this review. No secrets or unrelated project source were included in its scope.

The shared native contract suite passed all eight tests before closure. Target builds and VM tests were pending when this review completed; the [Contract runtimes workflow](../../.github/workflows/contracts.yml) supplies that separate execution evidence. This is a source review, not public-chain deployment or wallet acceptance.

The following is the final reviewer report:

I found no remaining material source defects. The one thing that can still overturn this is the target-VM CI, which hasn't run. This review was completed on Opus 5.5 (`claude-opus-5-5`). All 23 files in `/private/tmp/allowit-contract-final.sha256` match the worktree, and I didn't build or edit anything.

## Previous findings

- **M1 (stack offset):** closed. `contracts.yml:93-96` fails the job when the build log contains `Stack offset` or a stack frame that is too large. That catches the compiler warning that still produces a `.so`.
- **M2 (chain limits):** closed.
  - `validate_chain_artifact` (`common/src/lib.rs:20-80`) is the old Solana walker, unchanged, and still covers every expression and statement kind.
  - It now runs on both chains: Solana at activation (`solana/src/lib.rs:336`) and Stellar before storing the mandate (`stellar/src/lib.rs:177,208`).
  - The compiled-Wasm test now runs three policies: pass, user-input rollback, and the maximum-size semantic policy with signed evidence. It includes the check that a non-empty context without evidence returns `EvidenceRequired`.
  - I checked the pinned soroban-sdk 26.1.0: test environments enforce mainnet per-invocation resource limits by default (`env.rs:719`), and the test doesn't turn that off. So the Wasm run is held to mainnet-level limits, which is stronger than the default budget alone.
- **L1 (Solana transaction size):** closed.
  - `runtime_context` is capped at 256 bytes before any parsing (`solana/src/lib.rs:346`).
  - The new test builds a complete v0 transaction and asserts it is ≤ 1232 bytes. It includes three full signature slots, the compute-budget instructions, semantic evidence, a 256-byte context and an address lookup table. It checks size only, not real signatures, as you noted.
- **L2 (shared Stellar allowance):** closed. `README.md:73` documents the shared allowance.
- **L3 (context encoding):** closed. `common/src/lib.rs:362-366` requires the exact canonical re-serialization before the evidence check. The conformance tests reject a duplicate key, extra whitespace and unsorted keys.
- **Test gaps:** closed.
  - On the SBF VM: head creation that is unsigned or by another owner, re-initializing the head, a wrong head, a stale revision and a skipped revision.
  - The Solana Testnet case fails closed; CI runs it with `ALLOWIT_SOLANA_NETWORK=testnet` at `contracts.yml:43`.
  - The Stellar downgrade uses different envelope bytes and still fails with `BindingMismatch` because of the latest-revision check.

## Minor notes (none unsafe)

1. In the Stellar Wasm loop, the `EvidenceRequired` call isn't preceded by a budget reset. If budgets add up across calls, this could only make the test fail when it shouldn't. It can't make it pass when it shouldn't.
2. The packet-size test uses the fixture's short merchant, action and single interval. Longer values can still go over 1232 bytes. That fails closed and the README already tells clients to size the actual transaction.
3. `solana/tests/testnet.rs` quietly returns early on non-testnet builds. The real check is only the CI step at `contracts.yml:43`, so that step needs to stay.

## Still pending

- **CI:** none of the SBF build/Mollusk, Soroban Wasm or native runs have happened in CI yet. None of those unrun tests count as proof.
- **Agave checksum:** you verified it through the GitHub release API; I didn't re-check it independently.
- **Deployment:** there is still no public-chain deployment, wallet acceptance or live transaction.
