//! Merchant verification for cooperative, trusted Stellar HTTP binding guards.
//!
//! The caller must load configuration, current state, network and contract pins
//! from a fixed trusted RPC, never from payment payload fields. Keep the exact
//! final issued challenge in an authoritative quote store, capture the actual
//! raw HTTP body before parsing, and verify before executing a paid handler.
//! This module does not prove an arbitrary merchant deployed the guard, reserve
//! quotes, simulate settlement, or prevent an unguarded merchant accepting a
//! credential. The official facilitator must still simulate the same payment.

use super::*;

pub const HTTP_BINDING_PROFILE: &str = "allowit-stellar-http-binding-v1";
const MAX_URL_BYTES: usize = 8192;
const MAX_AUTH_LEDGER_WINDOW: u32 = 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub body: Vec<u8>,
}

/// Inspection alone is an untrusted quote-store lookup hint, not authorization.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaymentAuthorization {
    pub vault: String,
    pub request_hash: String,
    pub nonce: u64,
    pub revision: u64,
    pub expires_at: u64,
    pub signature_expiration_ledger: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpBindingQuote {
    pub profile: String,
    pub quote_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub merchant: String,
    pub action: String,
    pub verification_key: String,
    pub verifier_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttestationClaims {
    pub profile: String,
    pub merchant: String,
    pub action: String,
    pub origin: String,
    pub request_hash: String,
    pub challenge_hash: String,
    pub nonce: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub verifier_hash: String,
}

fn bounded_text(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err("Invalid bounded binding identifier".into());
    }
    Ok(())
}

fn canonical_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || value.bytes().all(|b| b == b'0')
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Expected 32-byte lowercase hexadecimal binding value".into());
    }
    Ok(())
}

/// Validate the supported quote declaration at a caller-supplied trusted time.
pub fn validate_http_binding_quote(offer: &Value, now: u64) -> Result<HttpBindingQuote> {
    if offer.pointer("/extra/paymentFlow") != Some(&json!("upfront")) {
        return Err("HTTP binding requires settlement before the paid handler".into());
    }
    let value = offer
        .pointer("/extra/allowitHttpBinding")
        .ok_or("Missing merchant HTTP binding profile")?;
    let quote: HttpBindingQuote = serde_json::from_value(value.clone()).map_err(error)?;
    if quote.profile != HTTP_BINDING_PROFILE
        || quote.issued_at == 0
        || quote.issued_at > now
        || quote.expires_at <= now
        || quote.expires_at <= quote.issued_at
        || quote.expires_at - quote.issued_at > 600
    {
        return Err("Unsupported, stale, or invalid HTTP binding quote".into());
    }
    canonical_hash(&quote.quote_id)?;
    canonical_hash(&quote.verifier_hash)?;
    bounded_text(&quote.merchant)?;
    bounded_text(&quote.action)?;
    account_key(&quote.verification_key)?;
    Ok(quote)
}

/// Shared canonical attestation digest; it does not itself establish freshness.
pub fn attestation_digest(claims: &AttestationClaims) -> Result<[u8; 32]> {
    if claims.profile != HTTP_BINDING_PROFILE
        || claims.issued_at == 0
        || claims.expires_at <= claims.issued_at
        || claims.expires_at - claims.issued_at > 60
        || claims.origin.len() > MAX_URL_BYTES
    {
        return Err("Unsupported or invalid merchant attestation window".into());
    }
    bounded_text(&claims.merchant)?;
    bounded_text(&claims.action)?;
    canonical_hash(&claims.nonce)?;
    canonical_hash(&claims.request_hash)?;
    canonical_hash(&claims.challenge_hash)?;
    canonical_hash(&claims.verifier_hash)?;
    validate_https_url(&claims.origin)?;
    let origin = Url::parse(&claims.origin).map_err(error)?;
    if origin.origin().ascii_serialization() != claims.origin {
        return Err("Attestation requires a canonical public HTTPS origin".into());
    }
    Ok(sha256(canonical_json(&json!({
        "domain":"allowit_stellar_http_attestation_v1", "claims":claims
    }))?))
}

/// Verify with a registry-pinned merchant G key, never a key from the claims.
/// The caller must compare all claims to expected request/challenge/identity,
/// require its fresh nonce and verifier pin, and enforce current-time expiry.
pub fn verify_merchant_attestation(
    claims: &AttestationClaims,
    signature_hex: &str,
    key_g: &str,
) -> Result<()> {
    verify_signature(
        key_g,
        &attestation_digest(claims)?,
        &unhex::<64>(signature_hex)?,
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WirePayment {
    x402_version: u32,
    resource: Value,
    accepted: Value,
    payload: WireTransaction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireTransaction {
    transaction: String,
}

struct ParsedPayment {
    wire: WirePayment,
    envelope: TransactionEnvelope,
    authorization: SorobanAuthorizationEntry,
    metadata: PaymentAuthorization,
    executor_signature: [u8; 64],
    authority_signature: [u8; 64],
}

fn parse_payment(credential: &str) -> Result<ParsedPayment> {
    if credential.is_empty() || credential.len() > MAX_BYTES * 2 {
        return Err("Payment credential exceeds limit or is empty".into());
    }
    let decoded = STANDARD.decode(credential).map_err(error)?;
    if decoded.len() > MAX_BYTES || STANDARD.encode(&decoded) != credential {
        return Err("Noncanonical or oversized payment credential".into());
    }
    let wire: WirePayment = serde_json::from_slice(&decoded).map_err(error)?;
    if wire.x402_version != 2 {
        return Err("Only Stellar x402 v2 payments are supported".into());
    }
    validate_offer(&wire.accepted, text(&wire.accepted, "network")?, u64::MAX)?;
    let envelope: TransactionEnvelope = read_xdr(&wire.payload.transaction)?;
    if write_xdr(&envelope)? != wire.payload.transaction {
        return Err("Noncanonical payment transaction XDR".into());
    }
    let TransactionEnvelope::Tx(tx) = &envelope else {
        return Err("Expected the published Stellar v1 payment envelope".into());
    };
    if tx.tx.operations.len() != 1 {
        return Err("Payment must contain one canonical SAC transfer operation".into());
    }
    let operation = &tx.tx.operations[0];
    if operation.source_account.is_some() {
        return Err("Payment operation source is not supported".into());
    }
    let OperationBody::InvokeHostFunction(op) = &operation.body else {
        return Err("Expected payment invokeHostFunction operation".into());
    };
    let HostFunction::InvokeContract(args) = &op.host_function else {
        return Err("Expected canonical SAC transfer invocation".into());
    };
    if op.auth.len() != 1 {
        return Err("Payment must contain exactly one vault authorization".into());
    }
    let authorization = op.auth[0].clone();
    let credentials = credentials(&authorization)?;
    let vault = address_text(&credentials.address)?;
    contract_hash(&vault)?;
    if args.contract_address != sc_address(text(&wire.accepted, "asset")?)?
        || args.function_name.0.as_slice() != b"transfer"
        || args.args.as_slice()
            != [
                ScVal::Address(credentials.address.clone()),
                ScVal::Address(sc_address(text(&wire.accepted, "payTo")?)?),
                ScVal::I128(Int128Parts {
                    hi: 0,
                    lo: offer_amount(&wire.accepted)?,
                }),
            ]
        || text(&wire.accepted, "payTo")? == vault
        || authorization.root_invocation.function
            != SorobanAuthorizedFunction::ContractFn(args.clone())
        || !authorization.root_invocation.sub_invocations.is_empty()
    {
        return Err(
            "Payment operation and vault authorization must match the exact SAC transfer".into(),
        );
    }
    let fields = sc_map(&credentials.signature)?;
    if fields.len() != 6 {
        return Err("Expected exactly six canonical vault signature fields".into());
    }
    let field = |name: &str| {
        fields
            .get(name)
            .ok_or_else(|| format!("Missing vault signature field {name}"))
    };
    let metadata = PaymentAuthorization {
        vault,
        request_hash: hex(&sc_bytes::<32>(field("request_hash")?)?),
        nonce: sc_u64(field("nonce")?)?,
        revision: sc_u64(field("revision")?)?,
        expires_at: sc_u64(field("expires_at")?)?,
        signature_expiration_ledger: credentials.signature_expiration_ledger,
    };
    let executor_signature = sc_bytes::<64>(field("executor_signature")?)?;
    let authority_signature = sc_bytes::<64>(field("authority_signature")?)?;
    Ok(ParsedPayment {
        wire,
        envelope,
        authorization,
        metadata,
        executor_signature,
        authority_signature,
    })
}

/// Extract structurally checked, untrusted metadata for quote lookup only.
/// A forged `requestHash` can pass inspection; `verify_http_payment` is required.
pub fn inspect_payment_authorization(credential: &str) -> Result<PaymentAuthorization> {
    Ok(parse_payment(credential)?.metadata)
}

/// Verify the exact incoming request and both signatures against trusted state.
/// Quote storage, pinned live contract validation, atomic reservation, handler
/// idempotency and real facilitator simulation are responsibilities of callers.
#[allow(clippy::too_many_arguments)]
pub fn verify_http_payment(
    c: &Config,
    s: &VaultState,
    r: &PaymentRequest,
    actual: &HttpRequest,
    credential: &str,
    now: u64,
    latest_ledger: u32,
) -> Result<PaymentAuthorization> {
    validate_state(c, s)?;
    validate_request(c, r)?;
    if actual.url.len() > MAX_URL_BYTES
        || r.url.len() > MAX_URL_BYTES
        || actual.body.len() > MAX_BYTES
        || actual.method != r.method
        || actual.url != r.url
        || actual.body != r.body
        || canonical_json(&r.payment_required)?.len() > MAX_BYTES
    {
        return Err(
            "Actual HTTP method, full URL, or raw body differs from the issued challenge".into(),
        );
    }
    let quote = validate_http_binding_quote(&r.selected_offer, now)?;
    let parsed = parse_payment(credential)?;
    if parsed.wire.resource != r.resource
        || parsed.wire.accepted != r.selected_offer
        || parsed.metadata.vault != c.vault
    {
        return Err(
            "Payment resource, selected offer, or vault differs from the issued challenge".into(),
        );
    }
    let expected_transfer = transfer_args(c, &r.selected_offer)?;
    let tx = transaction(&parsed.envelope)?;
    let OperationBody::InvokeHostFunction(op) = &tx.operations[0].body else {
        return Err("Expected payment transfer operation".into());
    };
    if op.host_function != HostFunction::InvokeContract(expected_transfer.clone())
        || parsed.authorization.root_invocation.function
            != SorobanAuthorizedFunction::ContractFn(expected_transfer)
        || !(100..=MAX_TRANSACTION_FEE_STROOPS).contains(&tx.fee)
    {
        return Err("Payment transfer or sponsored transaction fee is invalid".into());
    }
    match &tx.ext {
        TransactionExt::V1(data)
            if data.resource_fee >= 0 && data.resource_fee <= i64::from(tx.fee - 100) => {}
        _ => return Err("Payment Soroban resources or fee are invalid".into()),
    }
    let metadata = &parsed.metadata;
    let timeout = number(&r.selected_offer, "maxTimeoutSeconds")?;
    if !s.approved
        || metadata.nonce != s.nonce
        || metadata.revision != s.revision
        || s.nonce == u64::MAX
        || s.spent_day > now / 86400
        || metadata.expires_at <= now
        || metadata.expires_at > quote.expires_at
        || metadata.expires_at > now.checked_add(timeout).ok_or("Payment time overflow")?
        || latest_ledger == 0
        || metadata.signature_expiration_ledger <= latest_ledger
        || metadata.signature_expiration_ledger
            > latest_ledger
                .checked_add(MAX_AUTH_LEDGER_WINDOW)
                .ok_or("Ledger window overflow")?
    {
        return Err(
            "Payment authorization is stale, unapproved, or outside the current quote window"
                .into(),
        );
    }
    let spent = if s.spent_day == now / 86400 {
        s.spent
    } else {
        0
    };
    if spent
        .checked_add(offer_amount(&r.selected_offer)? / 10)
        .ok_or("Budget overflow")?
        > s.daily_limit
    {
        return Err("Payment exceeds the current policy budget".into());
    }
    let reconstructed = PaymentRequest {
        method: actual.method.clone(),
        url: actual.url.clone(),
        body: actual.body.clone(),
        resource: r.resource.clone(),
        payment_required: r.payment_required.clone(),
        selected_offer: r.selected_offer.clone(),
    };
    let request = request_hash(&reconstructed)?;
    if request != unhex::<32>(&metadata.request_hash)? {
        return Err(
            "Signed request hash differs from the actual HTTP request and original challenge"
                .into(),
        );
    }
    let host = host_digest(&c.network, &write_xdr(&parsed.authorization)?)?;
    let digest = authorization_digest(
        &c.vault,
        &host,
        metadata.nonce,
        metadata.revision,
        metadata.expires_at,
        &request,
    )?;
    verify_signature(&c.executor, &digest, &parsed.executor_signature)?;
    verify_signature(&c.authority, &digest, &parsed.authority_signature)?;
    Ok(parsed.metadata)
}
