# Native Rust demo codec

The codec compiles reviewed policy source and prepares exact Solana contract bytes. Each operation has one registered namespace and uses the shared compiler or rail library. It does not sign, submit transactions or own a settlement journal.

```sh
cargo build --locked --manifest-path contracts/client/Cargo.toml
printf '%s\n' '{"operation":"allowit::compile_policy","source":"pub async fn execute(ctx: &Context) -> PolicyResult { allowit::set_cap(ctx, \"5\", \"USDC\")?; Ok(()) }","originalIntent":"Small purchases"}' | contracts/client/target/debug/allowit-contract-client
```

`allowit::compile_policy` returns the compiled artifact and its exact hashes. `jev::preference_key` returns the evidence key for an exact question. The `solana::prepare_activation`, `solana::prepare_execution`, `solana::prepare_revoke`, `solana::prepare_state`, `solana::prepare_delegate`, `solana::decode_state` and `solana::derive_addresses` operations use the existing contract ABI. Their inputs retain the exact owner, account, revision and request bindings. Legacy `op` inputs remain accepted. A request must choose one dispatch field.

`paysh::inspect_request` accepts `requestBase64` containing the current interface's Borsh `Request`. It decodes concrete `PayUsdc` and `SwapSolToUsdc` actions and returns exact request identity, signing-message bytes and bound challenge/evidence hashes. The display labels `paysh::pay_usdc` and `paysh::swap_sol_to_usdc` identify these actions. They are not restricted-policy source functions. Inspection is not policy authorization or a transfer receipt.

Live execution uses the [native client and durable lifecycle](../../native-rust/src/lifecycle.rs) through the [AllowIt engine](https://github.com/AllowIt-hq/allowit-engine) and [action CLI](https://github.com/AllowIt-hq/allowit-cli): `allowit exec` and `allowit status`. That path verifies deployed program bytes, explicit network and owner authorization, binds request identity, records submitted bytes before broadcast, and reconciles finalized receipts. It owns recovery of uncertain transactions. This codec adds no second executor.

Use explicit Devnet configuration for the demo. Owner signatures must cover the exact prepared transaction. A request's claimed owner does not prove authority. Policy compilation does not prove deployment. See [contract setup and request bindings](../README.md) and [PaySH module boundaries](../../docs/native-vault-v2.md).

```sh
cargo test --locked --manifest-path contracts/client/Cargo.toml
cargo test --locked --manifest-path native-rust/Cargo.toml
```
