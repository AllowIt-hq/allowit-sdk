# Tempo request verification

This crate verifies native vault plans offline. It does not hold keys, call RPC,
broadcast transactions or prove delivery of an HTTP service.

`validate_execution_request` preserves the native payment protocol.
`validate_execution_request_mpp` additionally binds a `MppPaymentRequest` to the
same authority-signed execution tuple. Its version 1 envelope contains `method`,
the exact `url`, and the complete original JSON `challenge`, retaining the
encoded `request`, expiry and opaque extension strings.

The initial MPP scope is a public, canonical HTTPS GET URL without a query,
userinfo or fragment, Tempo testnet chain 42431, and a positive single-recipient
charge in the configured six-decimal token. Only push settlement with client-paid
fees is supported. Splits, pull-only requests, session/proof intents and unknown
charge fields are rejected. The action must be `GET <exact URL>` and the merchant
must be that URL's origin.

The memo follows mppx Attribution v1, with zero client attribution. An explicitly
supplied memo must match the original realm and challenge ID. Payment identity
includes the original challenge, HTTP request and raw caller context. Canonical
identity serialization is independent of serde_json's `preserve_order` feature.

Pass current time before authorizing or signing. `now = 0` is reserved for
verification of a saved historical plan during recovery; it does not authorize a
new payment. The signed approval cannot outlive the original challenge.

`MppPurchase` is a public payment summary, not evidence that a provider returned a
response. HTTP results and executor reports must be tracked separately from the
finalized native payment receipt.
