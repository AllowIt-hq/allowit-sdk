# Policy template

Replace `src/policy.rs` with the complete policy source. Keep `src/lib.rs` unchanged.

```sh
cargo test --locked --manifest-path templates/policy/Cargo.toml
cargo run --locked -- compile templates/policy/src/policy.rs
cargo run --locked -- evaluate templates/policy/src/policy.rs examples/context.json contract
```

The empty template builds and denies requests. Its `new()` constructor returns an empty configuration. The private `_execute` stub returns a policy rejection.

Fixed limits belong inside `_execute`. When the owner requests initialized configuration, declare its primitive fields in `PolicyParams` and set every value in `new()`. These constructor values are immutable. A change requires a newly approved source artifact. Constructor fields do not imply mutable storage.

The host template builds ordinary Rust and compiles the exact source through the real policy compiler. Its public `execute` evaluates checked IR with mandatory allocation checks. Solana SBF and Stellar Wasm use their native authenticated wrappers and separate VM tests. A host build is not a native deployment.
