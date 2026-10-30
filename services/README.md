# Allowit services

*One typed Rust facade for calling service vendors and rail protocols.*

`allowit-services` is the crate a server reaches for when a policy decision has to become a real call: a PaySH purchase, an x402 payment, a Tempo transaction. It gives those calls one import path and one set of types. The concrete protocol implementations live in their owning contract repositories; this crate re-exports them.

It stays deliberately small. It does not start a CLI process and it does not call the Allowit backend. The caller supplies deployment configuration and the bound operation data. The server keeps policy admission, spending limits, signer selection, journals and recovery decisions to itself.

## What is exported

| Module | Purpose |
| --- | --- |
| `upstream_from_env` | Select the configured PaySH service implementations from the environment |
| `catalog` | Discovery data for PaySH services |
| `stock`, `cooperating`, `x402` | The PaySH protocols |
| `rpc` | Bounded transports and typed rail requests, with `rpc::solana` and `rpc::tempo` |
| `tempo_transaction` | Native Tempo transaction encoding |
| `http` | The shared service transport |

## Develop

```sh
cargo fmt --manifest-path services/Cargo.toml --check
cargo clippy --manifest-path services/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path services/Cargo.toml --locked
```

The crate is split into [`core/`](core/) (`allowit-service-core`) and [`transport/`](transport/) (`allowit-service-transport`), each with its own manifest.
