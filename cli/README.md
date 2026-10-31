# Allowit CLI

*The `allowit` command: local agent keys, wallet setup, owner approvals, signed requests and the policy compiler tools.*

This crate (`allowit-cli`, library `allowit_cli`) builds the `allowit` binary. It orchestrates a request from the agent's side: it picks the rail, prepares and signs the operation with a key that never leaves the machine, validates receipts and recovers uncertain submissions. Backend requests, wallets and the service facade come from [`allowit-sdk-engine`](../engine/), which has no terminal I/O. Errors, configuration and private storage are the `client` module of [`allowit-sdk`](../core/), which also holds the compiler and evaluator. Rail crates live in their rail repositories and depend on the same core.

```sh
cargo run --locked --bin allowit -- help
cargo build --locked --release --bin allowit
```

Release builds use the workspace release profile (`lto`, `opt-level = "s"`, `panic = "abort"`, `strip`).

`--network` accepts x402 CAIP-2 identifiers. Explicit networks must match the selected wallet and policy deployment.
Solana addresses alone do not distinguish its clusters. EVM addresses alone do not identify a chain.

Source layout:

- `src/main.rs` is the binary. `src/lib.rs` runs a command with process-local environment and standard streams.
- `src/commands/<command>/` holds one top-level CLI command. Each subcommand has its own file. The
  policy compiler commands (`compile`, `check`, `workflow`, `evaluate`, `stellar-native`, `registry`,
  `json`, `lsp`) share `src/commands/compiler.rs` and run the SDK locally.
- `src/vendors/<name>/` holds each vendor connection: `paysh`, `solana`, `stellar` and `tempo`.
  Command code reaches a rail only through these modules.
- `src/gateway/` holds the input, operation and help helpers that the backend commands share. The
  backend transport is the engine gateway. `src/action/` holds the pipeline that `show`, `eval`, `exec`
  and `status` share.
- [`templates/policy/`](templates/policy/) is the copy-paste policy crate, and
  [`skills/allowit/`](skills/allowit/SKILL.md) is the agent skill.

Global flags can come before the command name, for example `allowit --json show POLICY` or
`allowit --json policy status`.

Generate an agent key locally:

```sh
allowit wallet generate --network solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1 --output /private/path/agent.json
allowit wallet show --network solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1 /private/path/agent.json
```

The parent directory must be private (0700). Existing files are never replaced. The private file is 0600.
Solana files use the native JSON keypair format. Tempo uses 32-byte hex. Stellar uses a StrKey secret seed.
Generation and inspection return only the network, address, public key and key type.
The server receives public registration metadata and operation signatures. It never receives these private files.
Private keys stay with the CLI user. Server validation and the server's independent authority remain separate.

`policy deploy AMOUNT` prints a browser signing URL and exits 10. Set `ALLOWIT_OWNER`,
`ALLOWIT_REQUEST_ID` and the public deployment, mint, executor and authority settings.
The fragment binds the exact policy, public wallet roles, network and amount. Keep the request ID for recovery.
The browser checks its pinned Devnet deployment and requires the matching owner wallet to review and sign.

Native Stellar Testnet uses two owner continuations:

```sh
allowit wallet configure --network stellar:testnet --owner G_OWNER --agent G_AGENT --engine G_ENGINE --request-id stellar-setup-001
allowit wallet approve --network stellar:testnet --policy-file public-policy.json --owner G_OWNER --request-id stellar-approval-001
```

`configure` prepares three native signer-setting operations for a fresh funded master-key account.
The account needs enough native balance for two additional signer reserves and the transaction fee.
The owner keeps administration authority. The agent and configured engine jointly authorize low- and
medium-threshold operations. The browser checks the configured engine identity and current account
before Freighter signs. It saves the exact signed XDR before submission. Recovery checks its original
hash and can resend only those saved bytes.
Repeating a setup request after its signing deadline returns the original link as recovery-only.
It never prepares replacement bytes under that request ID. Use a new request ID for an unsigned setup.

`approve` requires a policy already stored under the owner and a service-configured native installation.
The private owner-workspace preparation workflow supplies the policy JSON. Save the matching policy
object from `micro/policies` or `micro/generate`, not the response envelope, bare Rust source or executor bundle.
It binds the complete source, revision and public wallet binding. The browser compares those terms to
the stored policy before the existing owner message-signing flow. This approval moves no funds.
Native multisig checks signature thresholds; the engine enforces the approved policy off chain.
Public installation registration remains service configuration. Neither command sends an agent secret.
Generate the agent signer locally with `wallet generate`; use only its public address in setup.
After approval, export the public agent bundle in the browser. Set `ALLOWIT_STELLAR_BUNDLE` to that
file and `ALLOWIT_STELLAR_SIGNER` to the matching local signer file. `exec` and `status` read the key
locally. They send signed request proofs and public metadata. Do not set the legacy `ALLOWIT_TOKEN`
with these two variables.
The validated public bundle selects the Stellar network automatically. An explicit conflicting
`--network` fails before transport. `show POLICY` reads public wallet metadata without a signer or
server connection. `--source` includes the exact public source. Stellar `exec` requires a typed
`--request-file`. `status POLICY REQUEST_ID` recovers the original execution. Unsupported policy
lifecycle commands fail without using another rail.
Public `show` requires the public bundle. A legacy secret-bearing token is used only by `exec` and `status`.

For an existing Stellar policy, `policy deploy --network stellar:testnet` forwards to the approval
continuation. Set `ALLOWIT_STELLAR_POLICY_FILE`, `ALLOWIT_OWNER` and `ALLOWIT_REQUEST_ID`. Omit the
Solana funding amount. Both Stellar continuations print a public browser URL and exit 10.
A printed URL is a pending signing intent, not a finalized transaction.
Owner setup and approval journals belong to the current working directory. Keep that directory for recovery.

The backend's public executor configuration must match the CLI-generated agent public key before deployment.
A mismatched server executor, authority, token or deployment fails before signing. The CLI does not install another key silently.
The owner's approval binds a vault to that agent and the server's separate authority. Existing transaction verification
checks the requested effects before the local agent adds its signature.
Tests check secret exclusion from network registration values and local custody. They are not a formal security proof.

An expired unsigned browser setup needs a new `ALLOWIT_REQUEST_ID` and explicit
`ALLOWIT_RETRY_PREPARATION_OF` naming the original preparation. The browser checks authoritative expiry
and replacement permission. A signed uncertain transaction is never replaced through this option.

## Owner operations

`allowit_cli::admin::Admin<V>` delegates each operation to its bound vendor:
`deploy_policy(deployment_version, artifact)`, `set_pause(paused, deployment_version)` and
`recover_funds(token)`. The shared SDK defines separate capability traits without fallback implementations.
Vendor packages own native arguments, trusted state reads, authorization and transaction preparation.
Other configuration edits belong to the complete policy artifact and the same deployment flow.

The Stellar `stellar::admin::OwnerAdmin` prepares unsigned owner-review transactions for the versioned
policy wallet interface. Deployment and pause target exactly the active deployment version plus one. Recovery reads
the full selected token balance and current owner. It has no caller-supplied recipient, amount or nonce.
This profile recovers only the installed asset through the existing owner recovery entrypoint.
It does not implement arbitrary-token recovery or direct owner token authorization.

Preparation checks the network, pinned executable, current owner, policy binding, native authorization
scope and fee ceiling. It returns transaction XDR and required authorizers for native-wallet review and
signing. It does not submit transactions or produce a browser signing URL. This adapter requires a native
account as the current owner. Contract owners require a matching native authorization adapter.

`deployment_version` is the ordered owner-configuration counter. Deployment and pause use its next
value. `contract_version` identifies the policy contract code release. `engine_version` identifies the
engine code release. The first explicit contract and engine release values are each 1. Release numbers
are metadata; the exact executable hash identifies code.

The Stellar adapter supports contract release 1 and requires matching release metadata in the current
stored state and executing policy binding. New preparation uses `deployment_version`. Existing signed
`revision` packets from canonical SDK main retain a private decoder for status and recovery only.
They cannot authorize a new preparation or signature. Exact signed transaction bytes remain unchanged.

`PolicyDeployment.x402_enabled` is explicit owner opt-in. The contract derives the Ed25519 key
from the configured native G-agent address. The vendor validates that address and encodes the Boolean.
Contract agents cannot enable this native-key delegation.
This delegates the raw key; native account signer or threshold changes do not revoke it.
The owner disables x402 to revoke this delegated route.

`stellar::rpc::engine_version` is a separate read-only metadata query. It returns `None` only when host
diagnostics identify the missing getter on the selected engine. Network errors, malformed metadata
and other contract failures reject. Owner operations do not query the engine.

## Policy configuration and spent values

`stellar::admin::Grant` is a named map of native `ScVal` values. `NativeValues` preserves arbitrary
supported native values, including keys that no selected check reads. Its JSON form stores each value
as canonical base64 XDR. An installed asset is a separate immutable custody binding. Grant keys do
not activate checks automatically. `PolicyDeployment` has no pause field; deployment preserves the
current pause state, and `set_pause` changes it.

A native policy receives read-only Grant configuration and a working State containing Grant and spent.
It returns the exact final spent map to commit:

```rust
use allowit::v1::prelude::*;
async fn _execute(request: &Context, grant: &Grant, state: &State) -> Result<Spent, PolicyError> {
    let daily = allowit::daily_cap(request, state)?;
    let after_daily = allowit::with_spent(state, daily)?;
    let total = self::total_cap(request, &after_daily)?;
    let n = allowit::state_get(&total, "custom_count", 0u64)?;
    let final_spent = allowit::state_set(total, "custom_count", n + 1)?;
    Ok(final_spent)
}
```

`with_spent` replaces only spent and preserves Grant. `state_get` reads an expected scalar type;
`state_set` and `state_remove` return a new spent map. A property-only policy starts with
`allowit::spent(state)`. Custody commits only the map returned by the policy after successful effects;
returning an earlier map selects that earlier value. Denial, errors and failed token effects roll back
all changes. Policy maps cannot overwrite owner, asset, deployment version, execution nonce or replay
records. Rule accounting keys remain policy data, so owner code can intentionally change them.

The compiler recommends SimplePolicyEngine, CommonPolicyEngine or CustomPolicy from the whole source. Common uses an exact ordered subset of canonical checks. Custom uses local `allowit::` or `self::` mixins and preserves bespoke control flow. Shared-engine calls, cost selection and hybrid placement are rejected. The owner reviews the selected implementation, rule list, source identity and exact module hash.

`stellar::rpc::prepare` reads the current native Grant and full spent map, then simulates the exact
installed policy before new signing. It rejects archived, malformed and restoration-required
observations. A missing spent entry needs a native getter's successful None proof. Arbitrary request
JSON supplies neither these observations nor engine decisions. Classic/native wallets retain their
existing trusted accounting and local guard evaluation; a selected shared-engine call cannot run on
that route.

The server signs its native authorization after initial preflight. A second enforce-mode simulation checks the signed scope and resources before the agent receives the packet. Successful policy wallet preflight creates an opaque in-memory binding to the exact prepared operation.
`sign` rechecks that binding. Loading an unsigned intent from JSON requires fresh preflight under the
same logical operation ID. Existing signed bytes retain verification, status lookup and retry without
new signing approval. Preflight is an observation; execution applies the current native policy and
engine behavior. Common and Custom use native spent values. Simple requires durable service accounting and exact source evaluation before the server signs.

For new policy wallet signing, the command-line client reads the local `ALLOWIT_STELLAR_RPC_URL` setting.
It uses the Stellar vendor's `rpc::revalidate_prepared` against that RPC before signing the existing
gateway intent. The URL comes from local configuration, not the gateway bundle. The RPC receives
only public native observations and unsigned simulation bytes; it receives no token, seed or gateway
authorization header. HTTPS is required except for loopback HTTP. Missing configuration, stale state
or insufficient existing resources stops signing. Revalidation preserves the exact unsigned
transaction, hash and logical request ID. Classic execution and signed status/recovery do not require
this setting. If resource bounds cannot cover the current simulation, the client retains the original
intent and reports the failure; it does not replace the transaction or logical request ID.
