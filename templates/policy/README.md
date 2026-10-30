# Policy template

Replace `src/policy.rs` with the complete policy. Keep the public `execute` wrapper in `src/lib.rs`.

```sh
cargo test --locked --manifest-path templates/policy/Cargo.toml
cargo run --locked -- compile templates/policy/src/policy.rs
```

The default `_execute(ctx: &Context) -> PolicyResult` returns an ordinary `PolicyError`. The harness checks actual Rust and restricted source. It does not assume replacement policies must refuse execution.

Use explicit `allowit::` calls. Basic rules take one value or `grant::same_name`. Only the initial consecutive same-name Grant calls qualify for standard Stellar preflight. The server executes the full policy, including conditions and errors.

The host supplies authenticated operation facts, owner Grant and authoritative accounting. Public JSON does not supply these capabilities. A host build does not establish native deployment or settlement.
