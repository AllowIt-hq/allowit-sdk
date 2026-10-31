# `allowit-tempo`

*Verify, sign and deliver Tempo payments for an Allowit vault*

This crate is the Rust side of the [Allowit Tempo rail](https://github.com/allowit-hq/allowit-contracts-tempo). It owns plan verification, native `0x76` transactions, executor recovery, MPP challenge validation and paid HTTP delivery. The Allowit SDK CLI calls into it through `cli/vendors/tempo`. The crate uses shared client primitives from the `allowit-sdk` core crate (`client` feature) and never depends on the SDK CLI or engine.

## Two ways to build it

| Build | What you get |
| --- | --- |
| default | Offline verification only. No signing key is read and no network request is sent. |
| `--features client` | Executor transport, signing and a durable journal for recovery. Pulls in `allowit-sdk` (core, `client` feature), `mpp`, `reqwest` and friends. |

```sh
cargo test --locked -p allowit-tempo
cargo test --locked -p allowit-tempo --features client
```

> [!NOTE]
> On macOS, the journal rejects symlinked directory components, and `/var` is one. Point tests at a canonical temporary directory:
> `TMPDIR=/private/tmp cargo test --locked -p allowit-tempo --features client`

## What the offline API checks

The public verification functions take exact configuration, policy and transaction bindings and return a plain `Result`:

- `validate_policy` and `validate_prepared` bind a `Policy` and prepared `Transaction` to a `Config` for the `tempo-native-v1` profile.
- `validate_execution_request_service` binds normalized `ServicePaymentRequest` terms to the authority-signed transaction. The terms pin the HTTP request, network, asset, recipient, amount, memo and expiry. The action and merchant must match. Native identities stay unchanged when there are no service terms.
- `validate_execution_request_mpp` and `execution_request_identity_mpp` keep the MPP adapter's original `mppRequest` identity and challenge checks. These identities stay distinct from `servicePaymentRequest` identities.
- `validate_retained_execution_plan` re-checks a saved plan during recovery. Recovery verifies the original transaction and request; it never creates a replacement payment or challenge.

## The executor client

With `client` enabled, `run` takes Tempo operation arguments plus separate stdout and stderr buffers and returns an exit code. `run_with_request_id` does the same with an explicit request identity. `describe` reports a bundle's public policy schema.

The commands behind `run`:

| Command | What it does |
| --- | --- |
| `import` | Load an owner-exported, policy-scoped executor bundle. |
| `execute` | Evaluate the policy and validate the authority-signed plan. Prints the plan, spends nothing, exits 10. |
| `pay` | Validate the same plan, sign one native `0x76` vault call with the designated executor key and send it. The complete signed transaction is persisted before sending. |
| `call` | Perform one HTTPS GET, parse a real `402` Payment challenge with the official Rust MPP SDK, pay it through the same vault pipeline and submit the receipt once. |
| `resume` | Re-check an interrupted payment from its saved transaction. Never signs or authorizes anything new. |
| `submit` | Report a transaction hash for a plan the executor sent through its own wallet. |
| `status` | Look up a request that reached a payment operation. |

The transport supports Tempo testnet and localnet. MPP supports Tempo testnet (chain ID 42431) with charge or push and client-paid fees. Owner deployment, funding and recovery use browser signing, so no command here reads an owner or authority key. Only `pay` and `call`, the two commands that sign, read the executor key.

Configuration is all environment variables: `ALLOWIT_TEMPO_BUNDLE`, `ALLOWIT_TEMPO_EXECUTOR_KEY_FILE`, `ALLOWIT_TEMPO_RPC_URL`, `ALLOWIT_TEMPO_STATE_DIR`, `ALLOWIT_TEMPO_WAIT_SECONDS`, `ALLOWIT_TEMPO_MAX_GAS`, `ALLOWIT_TEMPO_MAX_FEE`, `ALLOWIT_REQUEST_ID` and a few more. [`src/tempo-usage.txt`](src/tempo-usage.txt) is the complete reference, including journal requirements, exit codes and the recovery rules worth reading before you run `pay` for real.

## Layout

```
src/lib.rs                 Types, digests and offline verification
src/service.rs             ServicePaymentRequest binding
src/mpp.rs                 MPP challenge parsing and validation
src/wallet.rs              Local secp256k1 key helpers, no I/O
src/config.rs              Request identity and environment lookup (client)
src/tempo.rs               Command dispatch and bundle handling (client)
src/tempo_pay.rs           Signing, sending, journal and recovery (client)
src/tempo_mpp.rs           402 challenge flow and HTTP delivery (client)
src/tempo_rpc.rs           RPC client and finality checks (client)
src/tempo_store.rs         Private journal scoped to executor and chain (client)
src/tempo_transaction.rs   Native 0x76 encoding (client)
src/tempo-usage.txt        Command and configuration reference
```

## License

MIT, see the repository [LICENSE](../../LICENSE).
