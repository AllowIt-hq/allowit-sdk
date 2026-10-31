# Solana host crates

When an agent pays through Solana, the on-chain programs in [allowit-contracts-solana](https://github.com/allowit-hq/allowit-contracts-solana) only see a signed request and move tokens. Everything around it, from the native vault lifecycle to discovering a service and building the HTTP call and the payment packet, happens off-chain. These crates are that off-chain half. The engine and the CLI in this workspace use them.

| Crate | Package | Role |
| --- | --- | --- |
| [`native`](native/src/lib.rs) | `allowit-native` | The Rust native SDK for the vault v2 lifecycle. The JavaScript counterpart is `@allowit/native` in the contracts repository. |
| [`solana-vendor`](solana-vendor/README.md) | `allowit-solana-vendor` | Host adapters and PaySH operation reconciliation. |
| [`service-upstreams`](service-upstreams/src/lib.rs) | `allowit-solana-service-upstreams` | Picks the configured provider from `ALLOWIT_PAYSH_UPSTREAM` (`google-air-quality` by default, or `open-meteo`) and validates service input. |
| [`provider-google-air-quality`](provider-google-air-quality/src/lib.rs) | `allowit-vendor-google-air-quality` | Air-quality lookups against Google's API. |
| [`provider-open-meteo`](provider-open-meteo/src/lib.rs) | `allowit-vendor-open-meteo` | Air-quality lookups against Open-Meteo. |
| [`paysh-catalog`](paysh-catalog/src/lib.rs) | `allowit-vendor-paysh-catalog` | The operator discovery catalog, shipped as a byte-pinned JSON snapshot. |
| [`delegated-x402`](delegated-x402/src/lib.rs) | `allowit-solana-delegated-x402` | Builds and checks standard x402 v2 SPL delegated-payment packets for Solana. |
| [`paysh-stock`](paysh-stock/src/lib.rs) | `allowit-vendor-paysh-stock` | The stock MPP challenge, funding, packet and HTTP protocol. |
| [`paysh-cooperating`](paysh-cooperating/src/lib.rs) | `allowit-solana-paysh-cooperating` | Fixed-origin HTTP transport for the cooperating PaySH gateway protocol. |
| [`service-rpc`](service-rpc/src/lib.rs) | `allowit-solana-service-rpc` | Typed Solana JSON-RPC request builders and the list of read-only methods. |

Two boundaries hold throughout:

- **No keys, no journal in the service crates.** The server keeps policy admission, spending limits, operation journals, signatures and recovery decisions. The service crates construct and check; they do not decide or sign.
- **Catalog entries grant nothing.** A service in the catalog is discovery metadata. Execution authority comes from the owner's PaySH configuration and the evaluator's signature.

None of this code runs inside an on-chain program.

## Build and test

Each crate is a member of the SDK workspace and uses the root lockfile:

```sh
cargo fmt --all --check
cargo clippy --locked -p allowit-solana-service-upstreams --all-targets -- -D warnings
cargo test --locked -p allowit-solana-service-upstreams
```

The `rails` job in [`fast.yml`](../../.github/workflows/fast.yml) runs clippy and the tests for every crate here.

## Licensing

Each crate declares its own license in `Cargo.toml`. `allowit-native` and `allowit-solana-vendor` are MIT. The service packages extracted from the Allowit engine carry `LicenseRef-Proprietary`.
