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
`deploy_policy(policy_version, artifact)`, `set_pause(paused, policy_version)` and
`recover_funds(token)`. The shared SDK defines separate capability traits without fallback implementations.
Vendor packages own native arguments, trusted state reads, authorization and transaction preparation.
Other configuration edits belong to the complete policy artifact and the same deployment flow.

The Stellar `stellar::admin::OwnerAdmin` prepares unsigned owner-review transactions for the versioned
ODC interface. Deployment and pause target exactly the active policy version plus one. Recovery reads
the full selected token balance and current owner. It has no caller-supplied recipient, amount or nonce.
This profile recovers only the installed asset through the existing owner recovery entrypoint.
It does not implement arbitrary-token recovery or direct owner token authorization.

Preparation checks the network, pinned executable, current owner, policy binding, native authorization
scope and fee ceiling. It returns transaction XDR and required authorizers for native-wallet review and
signing. It does not submit transactions or produce a browser signing URL. This adapter requires a native
account as the current owner. Contract owners require a matching native authorization adapter.

The vendor reads interface version 2 from `policy_binding()`, including after an upgrade with unmigrated
V1 storage. Existing delegated execution and retained receipts accept V1 `revision` packets and V2
`policy_version` packets. Historical serialized `TrustedState.revision` loads as `policyVersion` with
interface 1 by default. Unknown profiles, conflicting fields and changed packet bindings reject.
