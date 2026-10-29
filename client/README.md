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
