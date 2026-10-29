# AllowIt client SDK

The CLI calls this facade. Rail adapters live in vendor repositories and depend on shared SDK core packages.
`allowit-client-core` provides errors, configuration and private storage. The compiler/evaluator remains `allowit-sdk`.

`--network` accepts x402 CAIP-2 identifiers. Explicit networks must match the selected wallet and policy deployment.
Solana addresses alone do not distinguish its clusters. EVM addresses alone do not identify a chain.

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
A printed URL is a pending signing intent, not a finalized transaction.

The backend's public executor configuration must match the CLI-generated agent public key before deployment.
A mismatched server executor, authority, token or deployment fails before signing. The CLI does not install another key silently.
The owner's approval binds a vault to that agent and the server's separate authority. Existing transaction verification
checks the requested effects before the local agent adds its signature.
Tests check secret exclusion from network registration values and local custody. They are not a formal security proof.

An expired unsigned browser setup needs a new `ALLOWIT_REQUEST_ID` and explicit
`ALLOWIT_RETRY_PREPARATION_OF` naming the original preparation. The browser checks authoritative expiry
and replacement permission. A signed uncertain transaction is never replaced through this option.

## Owner operations

`allowit_client::admin::Admin<V>` delegates each operation to its bound vendor:
`deploy_policy(deployment_version, artifact)`, `set_pause(paused, deployment_version)` and
`recover_funds(token)`. The shared SDK defines separate capability traits without fallback implementations.
Vendor packages own native arguments, trusted state reads, authorization and transaction preparation.
Other configuration edits belong to the complete policy artifact and the same deployment flow.

The Stellar `stellar::admin::OwnerAdmin` prepares unsigned owner-review transactions for the versioned
ODC interface. Deployment and pause target exactly the active deployment version plus one. Recovery reads
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
    let total = engine::total_cap(request, &after_daily)?;
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

The compiler chooses placement at authoring time. Generic `allowit::` checks use the selected strategy;
`engine::` requests the shared contract, and `self::` requests an eligible bundled rule. Both use the
same canonical implementation and updated-spent result. System authority cannot be moved local by a
cheaper estimate. The placement manifest records the request, authority, decision and reason for each
check. Auto compares compatible, owner-supplied deployment and expected execution costs in one
normalized unit. Without compatible estimates or expected volume, it uses the stated responsibility
default. There is no runtime fallback. The owner reviews the manifest with the exact module hash.
A shared result that the policy adopts can replace the full spent map, including counters read by
bundled checks. Local describes implementation placement; it does not isolate accounting from
shared state writers. The manifest records this policy-wide trust boundary.

`stellar::rpc::prepare` reads the current native Grant and full spent map, then simulates the exact
installed policy before new signing. It rejects archived, malformed and restoration-required
observations. A missing spent entry needs a native getter's successful None proof. Arbitrary request
JSON supplies neither these observations nor engine decisions. Classic/native wallets retain their
existing trusted accounting and local guard evaluation; a selected shared-engine call cannot run on
that route.

Successful ODC preflight creates an opaque in-memory binding to the exact prepared operation.
`sign` rechecks that binding. Loading an unsigned intent from JSON requires fresh preflight under the
same logical operation ID. Existing signed bytes retain verification, status lookup and retry without
new signing approval. Preflight is an observation; execution applies the current native policy and
engine behavior. The SDK never treats classic-route usage fields as ODC accounting.

For new ODC signing, the command-line client reads the local `ALLOWIT_STELLAR_RPC_URL` setting.
It uses the Stellar vendor's `rpc::revalidate_prepared` against that RPC before signing the existing
gateway intent. The URL comes from local configuration, not the gateway bundle. The RPC receives
only public native observations and unsigned simulation bytes; it receives no token, seed or gateway
authorization header. HTTPS is required except for loopback HTTP. Missing configuration, stale state
or insufficient existing resources stops signing. Revalidation preserves the exact unsigned
transaction, hash and logical request ID. Classic execution and signed status/recovery do not require
this setting. If resource bounds cannot cover the current simulation, the client retains the original
intent and reports the failure; it does not replace the transaction or logical request ID.
