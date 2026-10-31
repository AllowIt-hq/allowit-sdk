# Policy template

*A Cargo crate to write your Allowit policy in, with your editor's help.*

Writing a policy should feel like writing Rust, because it is Rust. This template gives you a crate where `rustc`, rust-analyzer and the Allowit compiler all look at the same file. Replace [`src/policy.rs`](src/policy.rs) with your policy and keep [`src/lib.rs`](src/lib.rs) unchanged.

## Try it

```sh
cargo test --locked -p allowit-policy-template
cargo run --locked --bin allowit -- compile cli/templates/policy/src/policy.rs
cargo run --locked --bin allowit -- evaluate cli/templates/policy/src/policy.rs docs/examples/context.json contract
```

The shipped `src/policy.rs` is the smallest valid policy. It builds, and it denies every request:

```rust
use allowit::v1::prelude::*;

struct PolicyParams {}

fn new() -> PolicyParams {
    PolicyParams {}
}

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::fail("Policy is not configured")
}
```

## How to fill it in

- Put fixed limits inside `_execute`, as ordinary guards.
- When the owner wants named configuration, declare primitive fields on `PolicyParams` and set every value in `new()`. Those constructor values are immutable: changing one means a newly approved source artifact. Constructor fields never imply mutable storage.

## What the crate checks

`cargo test` builds the source as ordinary Rust and then compiles the exact same text through the real policy compiler. The crate's public `execute` evaluates the checked IR with mandatory allocation checks, so a policy that passes here is a policy the SDK accepts. Solana SBF and Stellar Wasm builds use their own native wrappers and VM tests; a host build here is not a native deployment.
