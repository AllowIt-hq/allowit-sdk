# Allowit SDK core

The `allowit-sdk` package (library `allowit_sdk`) defines the Allowit policy language. It compiles restricted Rust policy source to validated IR and evaluates that IR. The compiler and the evaluator do no I/O. Three feature-gated modules do: `lsp::run_stdio` (`compiler`) serves the language server on stdin and stdout, `client` (`client`) reads environment variables and local files, and `transport` (`transport`) makes blocking HTTP requests. The `engine/`, `cli/` and `rails/` crates in this workspace and the rail repositories depend on it.

## Modules

| Module | Feature | Contents |
| --- | --- | --- |
| crate root | | Policy types, IR validation, `evaluate_ir`, `process_value`, `process_json` and `registry` |
| `prelude` | | The names that policy source imports with `use allowit::prelude::*;` |
| `lsp` | `compiler` | The stdio language server (`lsp::run_stdio`). `compile` and `evaluate` at the crate root need the same feature. |
| `typed_workflow` | | Typed workflow records for hosts and services |
| `admin` | | Owner operations that a bound vendor implementation performs |
| `engine` | | The source type-checking facade for the shared onchain engine guards |
| `v1` | `std` | The versioned source facade that policy crates compile against |
| `spending` | `oracle-ledger` | Purchase-tier ledger support |
| `client` | `client` | Client configuration, errors, journals, output and private local storage |
| `service` | `service` | Rail-agnostic service operation contracts that vendor crates implement |
| `transport` | `transport` | Bounded blocking HTTP and JSON-RPC transports for service adapters |
| `contract` | `contract` | The chain contract artifact, mandate, execution state and binary IR transport that the rail contracts share |

## Features

| Feature | Purpose |
| --- | --- |
| `std`, `compiler`, `oracle-ledger` | The defaults |
| `typed-workflow` | Typed workflow records. `compiler` enables it. |
| `compact-contract-validation` | Error codes instead of diagnostics for `no_std` contracts |
| `client` | The `client` module. Enables `std`. |
| `service` | The `service` module. Enables `std`. |
| `transport` | The `transport` module. Enables `service`. |
| `contract` | The `contract` module. It builds without `std`. |
| `integer-json` | Integer-only JSON for contract artifacts. Enables `contract`. |
| `contract-stellar` | The Stellar chain profile: `integer-json` and `compact-contract-validation` |

With no features, the crate is the bare `no_std + alloc` interpreter.

## Binaries

`allowit-contract-artifact <policy.rs> <intent.txt> [--binary]` writes the contract artifact for a policy to stdout. It needs the `compiler` and `contract` features:

```sh
cargo run --locked -p allowit-sdk --features compiler,contract --bin allowit-contract-artifact -- docs/examples/approval.rs intent.txt
```

## WebAssembly

Run these commands from the repository root:

```sh
rustup target add wasm32-unknown-unknown
cargo rustc --locked -p allowit-sdk --release --lib --target wasm32-unknown-unknown --crate-type cdylib
node core/tests/test-wasm.mjs
```

The module is `target/wasm32-unknown-unknown/release/allowit_sdk.wasm`. It exports `alloc(len)`, `process(ptr, len)` and `dealloc(ptr, len)`.
