# Service SDK

Use this Rust library to call service vendors and rail protocols. It delegates concrete implementations to the owning contract repositories. It does not start a CLI process or call the AllowIt backend.

The caller supplies deployment configuration and bound operation data. The server owns policy admission, spending limits, signer selection, journals and recovery decisions.

`upstream_from_env` selects configured PaySH service implementations. `catalog` exposes discovery data. `stock`, `cooperating` and `x402` expose PaySH protocols. `rpc` exposes bounded transports and typed rail requests. `tempo_transaction` exposes native Tempo transaction encoding.

```sh
cargo fmt --manifest-path services/Cargo.toml --check
cargo clippy --manifest-path services/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path services/Cargo.toml --locked
```
