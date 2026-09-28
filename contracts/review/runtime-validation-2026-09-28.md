# Compiled contract validation — 2026-09-28

Source candidate: `20635a472d684fd75dfa253c64d65ee7cff027f5`.

Both jobs in [Contract runtimes 36453189230](https://github.com/ackrate/AllowIt-sdk/actions/runs/36453189230) passed. [SDK core CI 36453189144](https://github.com/ackrate/AllowIt-sdk/actions/runs/36453189144) also passed on that exact commit. SDK source, root manifest and root lockfile remain byte-identical to the separately reviewed core commit `80edc466dce493f371263f66a196d04c97ec355e`; only contract/workflow files changed.

These are real compiled target VM tests. They do not establish public-chain deployment, wallet acceptance, cryptographic wallet signatures, RPC submission, fees or live balances.

## Solana SBF

Artifact: `allowit_solana.so`, 649,440 bytes, SHA-256 `34d29dbdc2f724c226919d9bc4feef2c4d8c35deebcf21358d9061d71b0fc7b7`.

The build uses checksum-pinned Agave 4.3.0, cargo-build-sbf 4.3.0 and platform-tools v1.57, with explicit `ALLOWIT_SOLANA_NETWORK=devnet`. CI rejects unsafe stack-frame warnings even if the compiler emits an ELF. Mollusk 0.15.1 executes that ELF together with its real bundled SPL Token ELF and System Program. The heap is 256 KiB and the compute ceiling is 1,400,000 CU.

| Case | Measured compute |
| --- | ---: |
| Maximum 8,192-byte artifact / depth-eight semantic activation | 688,050 CU |
| Same policy, signed semantic evidence and real token transfer | 897,750 CU |
| Ordinary policy token transfer | 143,250–143,251 CU |

All eight SBF tests passed. They cover owner/compiler/executor/evidence signer requirements, PDA/head identity and revision changes, exact account/mint/asset/recipient/action/network bindings, real balances and allowance enforcement, allocation/nonce/replay/revocation, input rejection and rollback, attested semantic context, and the maximum admitted artifact. Six additional native tests and the separately selected Testnet canonical-asset rejection test passed.

The packet test serializes a complete v0 transaction with three 64-byte signature slots, compute-budget instructions, one semantic interval, a 256-byte runtime context and an address lookup table: **1,191 bytes**, below the 1,232-byte limit. This is packet-size proof, not verification of real signatures or network submission.

## Stellar Soroban

Artifact: `allowit_stellar.wasm`, 126,105 bytes, SHA-256 `348472e6f654dde394758b83a2238ed2e7d587ea0afa564bdaa23d83b2366ee3`.

Rust 1.98.0 targets `wasm32v1-none`; Soroban SDK 26.1.0 / host 26.1.3 uses its default mainnet invocation limits. The selected size configuration is LLVM `z` and checksum-pinned Binaryen 133 `-Oz --converge --strip-debug --strip-dwarf --strip-producers`, with MVP features. The official RustCrypto SHA-256 compact implementation is enabled only in the Stellar dependency graph, and standard empty/single-block/multi-block digest vectors pass.

Every listed operation starts with the unchanged default **100,000,000 CPU / 40 MiB memory** budget. Upload and deployment are distinct calls and measured separately; no unlimited budget or disabled invocation limit is used.

| Compiled-Wasm operation | CPU | Metered memory bytes |
| --- | ---: | ---: |
| Upload the exact artifact | 98,161,829 | 13,135,517 |
| Deploy the uploaded artifact | 961,752 | 2,973,759 |
| Ordinary policy activation | 10,069,469 | 1,850,262 |
| Ordinary real token transfer | 16,289,248 | 1,917,984 |
| Reached user input, rejected with state/balance rollback | 12,223,005 | 1,821,989 |
| Maximum 4,096-byte canonical artifact / depth-eight semantic activation | 29,017,830 | 1,856,182 |
| Same maximum policy, attested context and real token transfer | 50,270,158 | 1,937,341 |

All ten Soroban tests passed. One test uploads and deploys the actual Wasm and runs the three policy scenarios above; the other host/native tests cover authority, asset/scope/replay/budget/revocation, evidence signatures and freshness, semantic context, failed-transfer rollback, digest vectors, and rejection of an authenticated 4,097-byte canonical artifact. Tests use the real canonical Stellar Asset Contract in an isolated ledger, with mocked authorization rather than live wallet signatures.

Stellar uses the internal `ALITIR01` binary artifact transport but reconstructs the exact same SDK `Program` and canonical IR hash. Its raw binary and reconstructed canonical JSON are capped at **4,096 bytes**, with 256 nodes and depth eight. Solana retains 8,192 bytes. The Stellar limit was chosen after the 8 KiB semantic case exceeded the CPU ceiling; the new 4 KiB boundary is proven by this run, and the boundary is checked during activation and execution.

Runtime context stays JSON, capped at 1 KiB on Stellar (256 bytes on Solana). Stellar rejects fractional/exponent/out-of-range numeric values with `InvalidEvidence`; numbers must be canonical exact i64/u64 integers, including nested values. No numeric coercion, truncation or attestation normalization is applied. The bounded parser and binary decoder pass 15 shared tests with the Stellar feature; the default common profile passes 12 tests.

Upload has only about 1.8M CPU of headroom under this pinned host model. Any source, dependency or toolchain rebuild must pass the same compiled-artifact upload and execution gate. Deployment still requires network simulation, fee funding, explicit owner/compiler/executor/evidence authority configuration, canonical asset checks, and a recorded deployed code hash before the consumer can enable a program-controlled allowance.

## Review and retained evidence

The [initial contract source review](opus-5-5.md) and [Stellar transport/profile closure reviews](stellar-transport-opus-5-5.md) were completed through Claude Code using actual `claude-opus-5-5`. The final source candidate above has no remaining material review findings. Earlier reports' pending VM statements describe their review time; this successful target run supplies the later execution evidence. The earlier 8 KiB Stellar execution and upload failures remain in immutable CI history.

The CI artifacts `solana-sbf-evidence` and `soroban-and-native-evidence` contain the exact binaries, SHA-256 files, toolchain logs, build logs and runtime metrics. Deployment keypairs are excluded from artifact upload.
