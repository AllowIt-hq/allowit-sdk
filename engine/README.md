# Allowit SDK engine

*Backend requests and service composition for Allowit hosts and the `allowit` command, with no terminal I/O.*

`allowit-sdk-engine` (library `allowit_sdk_engine`) holds the request side of Allowit that does not depend on a terminal. Callers pass arguments and readers in and get values or `String` output back. The [`allowit` CLI](../cli/) adds argument parsing, standard streams and vendor command code on top. The compiler, evaluator and shared client, service and transport primitives are in [`allowit-sdk`](../core/).

## Modules

| Module | Purpose |
| --- | --- |
| `client` | The bounded HTTP client for the Allowit harness API, with single-send semantics for payment-bearing requests |
| `config` | Origin, policy and token configuration from the environment |
| `request` | Request flags, request bodies and derived request IDs |
| `typed_request` | Bounded typed request files and operation nonces |
| `output` | Result classification and printable result text |
| `skill` | Policy descriptions and network selection from them |
| `wallet` | Local agent wallets for Solana, Stellar and Tempo |
| `routing` | Network identifiers and the network selected for one command |
| `gateway` | The `/api/gateway/*` transport and its shared flag types |
| `services` | The typed service facade (feature `services`) |
| `error` | The shared error type, re-exported from `allowit_sdk::client::error` |

## Services facade

`allowit_sdk_engine::services` needs the `services` feature. It is the module a server reaches for when a policy decision has to become a real call: a PaySH purchase, an x402 payment, a Tempo transaction. It gives those calls one import path and one set of types. The service contracts and transports are in `allowit_sdk::service` and `allowit_sdk::transport`. The concrete protocols live in the host crates under [`rails/`](../rails/), and this module re-exports them.

It stays small. It does not start a CLI process and it does not call the Allowit backend. The `allowit` binary does not enable the feature, so the rail service crates are not linked into it. The caller supplies deployment configuration and the bound operation data. The server keeps policy admission, spending limits, signer selection, journals and recovery decisions to itself.

| Export | Purpose |
| --- | --- |
| `upstream_from_env` | Select the configured PaySH service implementations from the environment |
| `catalog` | Discovery data for PaySH services |
| `stock`, `cooperating`, `x402` | The PaySH protocols |
| `rpc` | Bounded transports and typed rail requests, with `rpc::solana` and `rpc::tempo` |
| `tempo_transaction` | Native Tempo transaction encoding |
| `http` | The shared service transport |

## Develop

Run these commands from the repository root:

```sh
cargo test --locked -p allowit-sdk-engine
cargo clippy --locked -p allowit-sdk-engine --all-targets -- -D warnings
cargo clippy --locked -p allowit-sdk-engine --features services --all-targets -- -D warnings
```
