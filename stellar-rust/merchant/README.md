# Stellar request binding for merchants

This adapter checks the actual HTTP request against the immutable vault's signed
request hash before the published x402 middleware can settle or run a handler.
It uses the native `allowit-stellar-http` verifier built from `stellar-rust`, the
published x402 2.28.0 Stellar scheme, canonical Circle USDC and a persistent
SQLite quote store. No vault digest, signature format or payment credential
format changes are required.

Build the native verifier with `cargo build --locked --release --bin
allowit-stellar-http`. Install this adapter's locked dependencies using `npm ci
--ignore-scripts`. Node 24.14.1 or later is required for `node:sqlite`.

Create `NativeVerifier` with its fixed absolute executable path, `QuoteStore`
with a persistent private POSIX database path, and `createRpcVaultLoader` with
the configured network, allowlisted RPC and exact immutable policy/vault pins.
Pass them to `createStellarMerchant` together with the initialized resource
server, server-only attestation key and explicit routes. Each route declares its
method, exact raw path, semantic action and seven-decimal token amount. Amounts
must be exactly representable as six-decimal budget units. Handlers receive
only the verified method, URL/query, raw body, action and request hash, and
return `{status: 200, body: ...}`. They cannot select actions from unbound
headers or cookies. Body routes require `application/octet-stream`; the current
engine provider supports GET with an empty body.

Install this middleware globally after trusted ingress/proxy configuration and
before body parsers or paid routes. Do not put an unguarded paid route ahead of
it. Incoming protocol/Host must match the configured public HTTPS origin;
forwarded Host, cookies, authorization, content encoding and unknown routing
headers are rejected. Proxy configuration must trust only the real ingress.
Raw request targets are checked without route alias or query normalization.

An unpaid request receives a normal sponsored exact 402 containing
`accepted.extra.allowitHttpBinding`. The complete final challenge is stored
durably before the response. A policy engine can retrieve that original quote
with `X-AllowIt-Request-Hash` and a fresh 64-hex
`X-AllowIt-Binding-Nonce`. The response contains the original `PAYMENT-REQUIRED`
and a short-lived signed `X-AllowIt-Request-Binding` attestation. Register the
profile, attestation public key and native executable SHA256 through the
engine's trusted merchant registry. A declaration or echoed client field is
insufficient.

On a paid request, the guard extracts the request hash from Soroban
authorization, checks the cached original request and current route, obtains
fresh trusted vault state/code pins, and verifies the exact transfer and both
signatures with the shared Rust implementation. It atomically claims the
original quote before entering x402. The supported `upfront` payment flow
settles before the handler. A consumed or uncertain claim survives restart and
is never replaced or rerun. Client recovery uses the original chain receipt;
it does not retransmit a paid request. Changing route semantics, code pins or
keys invalidates outstanding quotes.

This enforces the cooperating merchant acceptance boundary. It does not make
HTTP routing observable to the immutable on-chain vault. An arbitrary service
that deliberately accepts someone else's original credential without this
check is outside this trust boundary; a key/binary attestation authenticates a
trusted operator's deployment claim, not remote proof of program execution.
The engine must refuse unsupported merchants, and an operator must deploy the
guard on every paid acceptance route covered by that merchant's identity.

Run `npm test` with `ALLOWIT_STELLAR_HTTP_VERIFIER` pointing to the native binary.
Tests use deterministic public SDK fixtures and local HTTP listeners; their
facilitator responses are test doubles, not live settlement evidence.
