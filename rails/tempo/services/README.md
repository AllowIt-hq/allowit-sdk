# `allowit-tempo-services`

*Typed Tempo RPC requests and native `0x76` transactions for the Allowit server*

This small crate gives the Allowit server a Tempo vocabulary: typed JSON-RPC requests and one strict native transaction envelope. The server keeps every decision that matters, including policy admission, signer selection, journals, budgets and recovery. This crate only shapes the bytes. It builds on the `service` and `transport` modules of the `allowit-sdk` core crate from the [Allowit SDK](https://github.com/allowit-hq/allowit-sdk).

## What is inside

### `rpc::Request`

A `Request` implements the SDK's `Operation` trait and knows its own method and params. Constructors cover the calls the rail needs:

| Constructor | JSON-RPC method |
| --- | --- |
| `chain_id()`, `gas_price()` | `eth_chainId`, `eth_gasPrice` |
| `code(address, tag)` | `eth_getCode` |
| `call(address, data, tag)`, `call_from(from, to, data, value, tag)` | `eth_call` |
| `estimate_gas(tx)`, `estimate_gas_fields(...)`, `estimate_managed(...)` | `eth_estimateGas`, the last one as a full `0x76` envelope with `feeToken`, `nonceKey` and `keyType` |
| `block(tag, full)`, `block_number(number, full)` | `eth_getBlockByNumber` |
| `transaction(hash)`, `receipt(hash)` | `eth_getTransactionByHash`, `eth_getTransactionReceipt` |
| `nonce(address, tag)` | `eth_getTransactionCount` |
| `send_raw(wire)` | `eth_sendRawTransaction` |

`is_verification()` is true for every request except `send_raw`, so a caller can tell reads from the one write. `is_temporary_read_refusal` classifies RPC codes `-32002` and `-32005` as temporary, matching the rail's existing behavior.

### `transaction::NativeTransaction`

One self-paid EOA call, nothing more. The struct carries chain ID, fee fields, gas limit, nonce, destination, calldata and fee token. `sign(&key)` produces a `SignedTransaction` with raw bytes, hash and sender; `verify(&signed)` checks that a signed transaction decodes to exactly this envelope. `executor_address(&key)` derives the sender address.

The envelope deliberately refuses sponsorship, access-key authorization, delegation, additional calls, nonzero native value, nonce keys other than zero and noncanonical signatures. Calldata is capped at 64 KiB. Signed bytes are an executable authorization, so they stay out of diagnostics.

## Build and test

Rust 1.98 with `rustfmt` and `clippy`. The crate is a member of the SDK Cargo workspace and uses its lockfile:

```sh
cargo fmt --all --check
cargo clippy --locked -p allowit-tempo-services --all-targets -- -D warnings
cargo test --locked -p allowit-tempo-services
```

## Provenance and license

The transaction implementation is copied from the Allowit engine. The crate declares `LicenseRef-Proprietary`; the rest of this repository is [MIT](../../../LICENSE).
