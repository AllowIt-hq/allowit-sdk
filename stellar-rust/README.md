# AllowIt Stellar SDK

Native Rust preparation and verification for Stellar x402 v2 payments from an
AllowIt Soroban vault. This crate performs no HTTP requests, reads no key files,
and broadcasts no transactions. It uses the official `stellar-xdr` 28 types and
the existing vault's policy, nonce, revision, and six-decimal budget units.

The supported wire is sponsored `exact` on `stellar:testnet` or `stellar:pubnet`,
with canonical Circle USDC. Stellar USDC has seven decimals: the quote amount
`10000` means 0.001 USDC. Amounts must be divisible by ten to represent them exactly
in the vault's policy units.

## Configuration and live checks

`Config` pins the network, HTTPS RPC URL, vault, owner, executor, policy authority,
asset, policy address, vault WASM hash, policy WASM hash, and policy source hash.
Owner, executor, and authority must be separate G accounts. The vault and policy
are C addresses. `validate_config` checks identities and canonical asset binding.

Before signing, the caller must compare RPC `getNetwork` to
`network_passphrase(config.network)`, fetch the keys from `ledger_keys` with
`getLedgerEntries`, and call `validate_contract_entries`. The latter verifies
both contract-instance executable hashes and the corresponding WASM bytes.
Simulate `state_read_transaction` and pass the returned result XDR to
`parse_vault_state` and `validate_state`. Recheck the current state and remaining
budget before accessing either signer; an offline prepared plan is a snapshot.

## Prepare, approve, and sign

1. Fetch the exact purchase URL with redirects disabled. Preserve the original
   method and body bytes and the full `PAYMENT-REQUIRED` JSON.
2. `select_offer` selects a sponsored Stellar exact USDC quote under the caller's
   maximum amount in seven-decimal token units. Construct a `PaymentRequest`
   containing the method, URL, body, resource, full 402, and selected offer.
3. Simulate `transfer_transaction`. Require exactly one authorization entry
   belonging to the vault and no restoration preamble. `prepare_payment`
   verifies its token transfer and supports address credentials V1 and V2.
4. Set the authorization expiration ledger within the selected offer's timeout,
   using the network's current ledger-close estimate. Set the wall-clock expiry
   at most 300 seconds after preparation. Apply the returned Soroban resource
   data with `attach_simulation_data` if desired.
   Payments whose resource fee plus inclusion fee exceeds the published
   facilitator's 50,000-stroop ceiling are rejected before either signature.
5. An explicitly authorized policy operator calls `authority_sign` with its
   seed and saves the detached approval. This cryptographic approval represents
   the operator's policy decision; this crate does not perform semantic assessment.
6. The executor calls `verify_approval`, then `sign_payment` using only the
   executor seed and the detached approval. `SignedPayment.payment_signature`
   is the standard base64 x402 `PAYMENT-SIGNATURE` value.

Both signatures cover the SHA-256 digest of the exact Soroban ScVal tuple:

```text
[Symbol("allowit_x402_v1"), Address(vault), Bytes32(host_auth_digest),
 U64(nonce), U64(revision), U64(expires_at), Bytes32(request_hash)]
```

The host digest binds the network, Soroban nonce, ledger expiry, and exact token
invocation; V2 additionally binds the authorization address. The request hash
binds the exact HTTP request and original payment challenge. The authorization
signature ScMap contains `authority_signature`, `executor_signature`,
`expires_at`, `nonce`, `request_hash`, and `revision` in canonical key order.

`validate_prepared` detects changes to the request, quote, state, digest, and
unsigned operation. `validate_signed` additionally verifies both signatures,
the complete saved payload, and its encoded header without rereading keys.

## Confirm and recover

`parse_payment_response` validates the provider's reported settlement identity.
It does not prove settlement. Query `getTransaction` for the receipt hash, then
call `validate_receipt`. Confirmation requires SUCCESS, a transaction envelope
whose hash matches the receipt on the configured network, the exact signed
transfer and authorization, a successful result XDR, and an exact USDC transfer
event in Soroban V3 or V4 metadata. Fee-bump envelopes are supported.

The facilitator replaces the client's transaction source, sequence, and fees;
the resulting hash differs from the prepared unsigned envelope. Receipt checks
therefore bind the actual confirmed envelope, operation, authorization, and event.
They remain available after authorization expiry for recovery. Save the signed
credential before sending it. An HTTP timeout or missing receipt is ambiguous
and must be reconciled without constructing a fresh payment.

## Verification

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cd tests/node
npm ci --ignore-scripts
npm test
```

The independent Node checks use pinned official `@stellar/stellar-sdk` and
`@x402/stellar`. They verify Rust's C-account envelope, canonical tuple digest,
both Ed25519 signatures, standard payload, and facilitator-rewritten receipt.
They also execute the published facilitator verifier for both credential arms,
including a successful authorization and token event exported from a real
compiled Soroban vault host test. RPC plumbing and resource fees are mocked;
these tests establish protocol compatibility without claiming a live settlement.
All fixture keys and bytecode are public test material; the mock vault fixture
is not deployable and no tests perform payments or network requests.
