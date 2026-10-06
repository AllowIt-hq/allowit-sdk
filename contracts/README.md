# AllowIt contract rails

These adapters enforce a compiled AllowIt mandate and perform a bounded token transfer in the same chain transaction. They use the SDK's deterministic `evaluate_ir` implementation with `Profile::Contract`. A reached `require_user_input` always returns `UserInputRequired`; there is no answer or oracle-receipt bypass in either contract API.

`common` owns the mandate envelope, artifact checks, exact request binding, budget/nonce accounting, evidence checks and host-side artifact command. `solana` is a native Rust program using classic SPL Token `transfer_checked`. `stellar` is a Soroban Rust/Wasm contract using SEP-41 `transfer_from`. Each package has a separate Cargo lockfile and build root.

## Artifact and authority boundary

The owner chooses an explicit compiler authority and executor when activating a mandate. The compiler must compile the reviewed source with this SDK and authenticate the resulting artifact. The owner and compiler both authorize the **same complete mandate and artifact**. There is no default compiler key or implicit global trust root.

The chain verifies compiler authorization, canonical IR hash, artifact hash, registry/core/compiler versions, structural/type/effect limits and every immutable envelope field. It does **not** run the host `syn` Rust parser. The compiler attestation is therefore the trust boundary for source-to-IR correspondence. A signer service must compile the owner-reviewed source itself; it must never attest an arbitrary caller-supplied IR/source pair. Owner authorization selects that compiler key and its key ID/version. Rotating authority or changing source, intent, scope or limits requires a newly activated mandate.

The immutable envelope binds a nonzero logical `policy_id`, owner, executor, compiler key/ID/version, optional evidence authority, registry/core versions, network, exact asset, asset decimals, recipient, action, merchant, revision, expiry, allocation, source hash, canonical IR hash and complete artifact hash. The artifact also stores the immutable original intent. `artifact_hash` covers that intent, and every evidence request hash covers the complete mandate and artifact hash.

Each owner/policy pair has one active revision. The first revision is 1; activation requires the previous revision plus one and immediately prevents older revisions from spending, including against an existing token allowance. Forks use a new `policy_id`. Revocation is permanent for a revision, and changing any immutable field requires a new activation.

Create exact artifact bytes from source and owner intent:

```sh
cargo run --manifest-path contracts/common/Cargo.toml --features compiler \
  --bin allowit-contract-artifact -- policy.rs intent.txt > artifact.json
```

For Stellar, use `--features compiler,stellar` and append `--binary` to emit the internal `ALITIR01` binary artifact instead of JSON. The SDK and CLI runtime-context interface remains JSON; only the stored Stellar policy artifact uses binary transport. `allowit_contract_core::binary::encode` converts the compiled artifact to this wire format. Both formats reconstruct the exact same SDK `Program`, source spans and canonical IR hash. The artifact hash covers the chosen exact wire bytes, so JSON and binary mandate IDs differ.

`ALITIR01` has an eight-byte version marker, little-endian fixed-width integers, u16 byte lengths/counts, UTF-8 strings, exact boolean/optional tags and exhaustive expression/statement tags. The decoder rejects unknown versions/tags, malformed lengths/UTF-8, trailing bytes, depth above eight and more than 256 nodes before recursive allocation. The reconstructed canonical JSON artifact must fit Stellar's 4,096-byte profile (Solana permits 8,192 bytes), so binary transport does not expand the admitted policy size. `validate_binary_artifact` rechecks all metadata and the canonical SDK IR hash; `prepare_binary_execution` uses the same shared deterministic evaluator and bindings as JSON execution.

This command compiles source and emits exact bytes without a trailing newline. Set `artifact_hash` to SHA-256 of those exact bytes. Use `allowit_contract_core::Mandate`, `Request`, `mandate_hash` and `request_hash` when constructing client payloads. Borsh encoding, enum discriminants and the pinned version are part of this first wire protocol; arbitrary JSON is not an instruction.

## Requests and evidence

The executor authorizes the exact request. Every request binds the current mandate revision and source/IR hash, asset, recipient, network, action, merchant, atomic amount and monotonically increasing nonce. The wrapper checks revocation, expiry and remaining allocation independently of policy source. It cannot make arbitrary program/contract calls or sign arbitrary data. Its only spending operation is the bound token transfer.

`merchant` and `action` are owner-approved metadata authenticated by the executor. Neither blockchain can independently verify an off-chain merchant's commercial claims. The actual token, source owner, destination and amount are checked against on-chain accounts/addresses.

Confidence and semantic assessments require an **explicit evidence authority**, separately configured in the owner/compiler activation. The same key can fill both executor and evidence roles only when the owner explicitly chooses it. An evidence snapshot binds all request bytes and the full mandate, including original intent, nonce and complete runtime-context JSON. It carries authority key ID/version, issue/expiry times (at most five minutes), and up to four named intervals. Inverted or out-of-range bounds, duplicate names, stale evidence, wrong authority and substituted requests fail. `semantic` uses the SDK's SHA-256 question key. A provider's point score may be represented by equal bounds; this does not make it a statistically calibrated interval.

Runtime context is an object of at most 256 bytes on Solana and 1 KiB on Stellar, subject to the SDK's nesting/type bounds. Stellar uses a bounded integer-only JSON parser and string-backed JSON numbers because Soroban forbids floating-point instructions; context numbers must be exact signed/unsigned 64-bit integers (no fractions, exponents or larger numbers), including nested values. Strings, booleans, nulls, arrays and objects retain their JSON representation. Its encoding must exactly match `serde_json::to_string` of the parsed value: sorted object keys, no redundant whitespace or duplicate keys. Nonempty runtime context requires evidence authorization. The core receives the immutable original intent from stored artifact bytes; an executor cannot replace it. These are smaller limits than the headless engine because chain transactions have finite payload and compute budgets.

The policy core measures USDC in six decimal places. These rail adapters accept only Circle's canonical USDC mint/asset on the chosen network, from its [official registry](https://developers.circle.com/stablecoins/usdc-contract-addresses). Solana checks the six-decimal classic SPL mint (`EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` on Mainnet, `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU` on Devnet). No canonical USDC mapping exists for Solana Testnet, so USDC mandate activation on that rail fails closed. A custom token cannot inherit the USDC label.

Stellar derives the canonical network-specific Stellar Asset Contract address from `USDC` and Circle's issuer: `GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN` on Mainnet and `GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5` on Testnet. It checks seven decimals and converts atomic amounts exactly to six-decimal policy units. Sub-micro-unit dust is rejected, never rounded downward.

## Fixed Devnet paid-API vault

The separate [vault v1 interface](solana/VAULT.md) supports a funded PDA wallet with fixed hard limits and per-challenge replay markers. It does not execute arbitrary IR and does not change the allowance API below.

## Solana API

The program has no hardcoded program ID or default authority. A deployment chooses its program ID. Build a separate artifact for each explicit network:

```sh
ALLOWIT_SOLANA_NETWORK=devnet cargo build-sbf \
  --manifest-path contracts/solana/Cargo.toml --sbf-out-dir "$PWD/contracts/dist"
```

Use `mainnet`, `testnet` or `devnet`. The deployable SBF build refuses an absent/unknown network setting. Solana does not expose a genesis-hash sysvar to programs: the deployment process must check RPC genesis and record the matching artifact/program ID on each cluster. Native tests use Devnet as their fixture network.

`Instruction` is Borsh encoded; accounts are documented beside each variant in `solana/src/lib.rs`:

1. Call `InitializeHead { policy_id }` once with the owner signer/payer, Rent, System Program and `head_address(program_id, owner, policy_id)` PDA. The program creates its 48-byte revision head with System CPI. A prefunded PDA is accepted.
2. The owner funds/creates a fresh program-owned, rent-exempt `STATE_BYTES` account. `Initialize` requires that account's signature, the owner signature, Rent and the revision head. It stores the immutable mandate in draft state.
3. `Upload` appends artifact chunks of at most 700 bytes, in order. Only the owner may upload; no modification is possible after activation or revocation.
4. `Activate` requires owner and compiler signatures, Clock, the writable revision head, and the exact `mandate_hash`. Both signatures cover this hash in the transaction. The program validates the uploaded artifact and advances the active revision atomically.
5. The owner approves an SPL Token allowance to `delegate_address(program_id, mandate_account)`, a PDA derived from `b"allowit"` and the mandate account. Tokens remain in the owner's token account.
6. `Execute` requires the bound executor signer and exact source/destination/mint accounts, Clock and the revision head. If evidence is supplied, the configured evidence authority must also sign the complete instruction; its account follows the revision head. The program verifies the active revision, PDA delegate, classic Token Program ID, actual token owners/mints/decimals and allowance, evaluates, then invokes only `transfer_checked` with PDA signing. It records spent amount and nonce after the transfer.
7. `Revoke` requires the owner and permanently disables this mandate account. The owner can independently revoke the wallet's SPL allowance. The program never closes/reuses a mandate account or resets its nonce.

Prepend `required_compute_budget_instructions()` to program transactions: the compiled allocator requires a 256 KiB heap, and tests enforce a 1,400,000 CU ceiling. Solana artifacts are limited to 8,192 bytes; Stellar binary and reconstructed canonical JSON artifacts are limited to 4,096 bytes. Both profiles allow at most 256 IR nodes and depth eight. Stellar's smaller profile is enforced at activation and is based on measured execution cost under its 100M CPU limit. The SDK uses Solana's SHA-256 syscall on the SBF target. Instruction data is capped at 1,024 bytes in addition to the full 1,232-byte transaction limit. The packet-size test serializes a complete v0 transaction with three 64-byte signature slots, compute-budget instructions, one semantic interval, a 256-byte context and an address lookup table. Clients must check the actual serialized transaction: longer metadata or additional intervals reduce the available context space. A source token account has one classic SPL delegate; approving another policy delegate replaces that account's previous delegation.

## Stellar API

```sh
cargo build --release --locked --target wasm32v1-none \
  --manifest-path contracts/stellar/Cargo.toml
```

Optimize the Wasm with checksum-pinned Binaryen 133 using the commands in `contracts.yml`. CI compares LLVM size profiles `z`/`s` and Binaryen `-Oz` plus optimization levels 2/4 at shrink level 2, logs the selected profile, and verifies the smallest artifact. Run the exact optimized bytes through the compiled-Wasm test before deployment. Stellar enables the pinned RustCrypto SHA-256 compact implementation; standard hash vectors and real-VM artifact validation check identical digest behavior. Upload costs count toward the network budget too; raw build output is not an accepted deployable artifact.

Deploy the verified `allowit_stellar.wasm` using the chosen Stellar account/network. No constructor installs a default authority.

- `activate(Activation) -> BytesN<32>` requires owner and compiler authorization covering every argument. It checks all bound addresses, actual token decimals, and the ledger network ID, then stores the immutable mandate and returns SHA-256 of its Borsh envelope as its ID.
- The owner grants a SEP-41 allowance to the contract address. Funds stay in the owner's token account. This allowance is shared by all of that owner's active policies for this token; each policy still has its own signed allocation and spent counter. Cancelling the shared allowance stops all those policies.
- `execute(id, request_bytes) -> spent_atomic_units` requires the executor. Evidence additionally requires the bound authority's `require_auth_for_args` over the exact `(id, request_bytes)`. After evaluation, the contract calls only the bound token's `transfer_from` to the bound recipient. Failed policy checks and failed token calls leave counters/balances unchanged through transaction rollback.
- `revoke(id)` requires the owner and permanently disables that stored mandate. The owner may separately cancel the token allowance.
- `state(id)` returns read-only Borsh state for clients.

The network ID is checked against the actual Stellar Public or Test Network passphrase hash, yielding `stellar:mainnet` or `stellar:testnet`. Solana uses `solana:mainnet`, `solana:devnet` or `solana:testnet`. These labels match the consumer's immutable policy network. Stellar addresses are bound as SHA-256 of their XDR address encoding; the human-readable recipient string is independently checked. Persistent storage retains the latest revision and nonce/revocation state, extends TTL, and must be restored if archived. Archival does not create a fresh active mandate.

## Verification and deployment boundary

The [2026-09-28 target validation receipt](https://github.com/ackrate/ackrate-project/blob/main/instance/artifacts/107-allowit-repository-artifacts/AllowIt-sdk/contracts/review/runtime-validation-2026-09-28.md) records passing compiled SBF and Soroban VM runs, exact artifact hashes, resource measurements and the Opus 5.5 source-review identities.

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --locked --manifest-path contracts/common/Cargo.toml
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --locked --manifest-path contracts/solana/Cargo.toml
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --locked --manifest-path contracts/stellar/Cargo.toml
```

The common vectors compare oracle and contract core outcomes/error codes, then test bindings, monetary boundaries, replay/revocation, confidence, semantic context and input rejection. Native Solana tests dispatch signed CPI to the actual SPL Token processor and assert balances. Soroban tests use the actual Stellar Asset Contract and authorization host, including rollback when transfer fails.

[Contract runtimes CI](../.github/workflows/contracts.yml) builds the actual SBF program with checksum-pinned official Agave 4.3.0/platform-tools 1.57, runs it with the real SPL Token ELF and System Program in Mollusk 0.15.1, and uploads the ELF/hash/build logs/CU results. The maximum admitted artifact and depth-eight policy must activate and transfer within the configured resource limit. To run that suite after building:

```sh
SBF_OUT_DIR="$PWD/contracts/dist" ALLOWIT_SOLANA_NETWORK=devnet \
  cargo test --locked --manifest-path contracts/sbf-tests/Cargo.toml -- --nocapture
```

The other CI job compiles Soroban Wasm and sets `ALLOWIT_STELLAR_WASM` to execute that exact artifact in the Soroban VM under the default budget, with CPU/memory metrics. A missing Wasm path is an error in CI. Uploaded artifacts and logs are tied to the source commit; a build alone does not establish successful VM execution.

These tests are not evidence of public-chain deployment or wallet acceptance. Deployment must record program/contract and artifact hashes, target-VM/resource results, network, token and authority configuration before the consumer uses a program-controlled allowance. On Solana, deploy the reviewed program immutably or explicitly verify the upgrade authority: that authority can otherwise replace code controlling PDA allowances. No private key or live-chain transaction is included here.
