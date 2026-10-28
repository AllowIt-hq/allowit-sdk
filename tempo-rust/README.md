# Tempo request verification

This crate verifies native vault plans offline. It does not hold keys, call RPC,
broadcast transactions or prove service delivery.

`validate_execution_request` preserves the native payment protocol.
`validate_execution_request_service` binds a version 1 `ServicePaymentRequest`
to the authority-signed execution tuple. The normalized terms include the exact
HTTP method, URL and origin, original request digest, chain, asset, recipient,
base-unit amount, memo and optional expiry. Vendor adapters decode and validate
their original challenges before producing these terms.

The SDK checks exact payment terms and signatures. The action must name the
exact method and URL. The merchant must match the URL origin. Payment identity
includes the normalized terms and raw caller context. Without service terms,
the native request identity remains unchanged. New service identity uses the
`servicePaymentRequest` key and does not reinterpret older protocol identities.

Use current time before authorizing or signing. The signed approval cannot
outlive the service terms. Keep delivery results separate from chain receipts.

`validate_retained_execution_plan` verifies historical signed effects against
the original request hash and memo from a durable approved record. It does not
compute a new service identity or grant new authorization. Vendor adapters retain
original protocol checks for historical recovery. Never re-sign an old plan under
a new identity or derive its expected binding from unchecked packet fields.
