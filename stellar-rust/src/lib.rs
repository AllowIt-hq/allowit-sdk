//! Offline Stellar x402 preparation, authorization, and receipt verification.
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use stellar_strkey::{ed25519, Contract, Strkey};
use stellar_xdr::*;
use url::Url;

pub type Result<T> = std::result::Result<T, String>;
pub const PROFILE: &str = "stellar-native-x402-v1";
pub const USDC_PUBNET: &str = "CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75";
pub const USDC_TESTNET: &str = "CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA";
const MAX_BYTES: usize = 1_048_576;
/// Default published @x402/stellar facilitator's total transaction-fee ceiling.
pub const MAX_TRANSACTION_FEE_STROOPS: u32 = 50_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub network: String,
    pub rpc_url: String,
    pub vault: String,
    pub owner: String,
    pub executor: String,
    pub authority: String,
    pub asset: String,
    pub policy: String,
    pub vault_wasm_hash: String,
    pub policy_wasm_hash: String,
    pub policy_source_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VaultState {
    pub owner: String,
    pub executor: String,
    pub authority: String,
    pub asset: String,
    pub policy: String,
    pub policy_source: String,
    pub policy_wasm: String,
    pub daily_limit: u64,
    pub spent: u64,
    pub spent_day: u64,
    pub nonce: u64,
    pub revision: u64,
    pub approved: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaymentRequest {
    pub method: String,
    pub url: String,
    pub body: Vec<u8>,
    pub resource: Value,
    pub payment_required: Value,
    pub selected_offer: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedPayment {
    pub config: Config,
    pub state: VaultState,
    pub request: PaymentRequest,
    pub transaction_xdr: String,
    pub authorization_xdr: String,
    pub request_hash: String,
    pub host_digest: String,
    pub authorization_digest: String,
    pub signature_expiration_ledger: u32,
    pub expires_at: u64,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetachedApproval {
    pub authority: String,
    pub digest: String,
    pub signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedPayment {
    pub payload: Value,
    pub payment_signature: String,
    pub transaction_xdr: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementReceipt {
    pub transaction: String,
    pub payer: String,
    pub network: String,
}

fn limits() -> Limits {
    Limits {
        depth: 64,
        len: MAX_BYTES,
    }
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn sha256(bytes: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(bytes.as_ref()).into()
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex<const N: usize>(s: &str) -> Result<[u8; N]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid fixed-length hexadecimal value".into());
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(error)?;
    }
    Ok(out)
}
pub fn network_passphrase(network: &str) -> Result<&'static str> {
    match network {
        "stellar:pubnet" => Ok("Public Global Stellar Network ; September 2015"),
        "stellar:testnet" => Ok("Test SDF Network ; September 2015"),
        _ => Err("Use stellar:pubnet or stellar:testnet".into()),
    }
}
pub fn canonical_asset(network: &str) -> Result<&'static str> {
    match network {
        "stellar:pubnet" => Ok(USDC_PUBNET),
        "stellar:testnet" => Ok(USDC_TESTNET),
        _ => Err("Unsupported Stellar network".into()),
    }
}
pub fn account_for_seed(seed: &[u8; 32]) -> String {
    ed25519::PublicKey(SigningKey::from_bytes(seed).verifying_key().to_bytes()).to_string()
}
pub fn decode_seed(value: &str) -> Result<[u8; 32]> {
    ed25519::PrivateKey::from_string(value)
        .map(|k| k.0)
        .map_err(|_| "Invalid Stellar secret seed".into())
}
pub fn sc_address(value: &str) -> Result<ScAddress> {
    match Strkey::from_string(value).map_err(|_| "Invalid Stellar address")? {
        Strkey::PublicKeyEd25519(key) => Ok(ScAddress::Account(AccountId(
            PublicKey::PublicKeyTypeEd25519(Uint256(key.0)),
        ))),
        Strkey::Contract(key) => Ok(ScAddress::Contract(ContractId(Hash(key.0)))),
        _ => Err("Only Stellar G or C addresses are supported".into()),
    }
}
pub fn address_text(address: &ScAddress) -> Result<String> {
    match address {
        ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(key))) => {
            Ok(ed25519::PublicKey(key.0).to_string())
        }
        ScAddress::Contract(id) => Ok(Contract(id.0 .0).to_string()),
        _ => Err("Unsupported Stellar address type".into()),
    }
}
fn account_key(address: &str) -> Result<[u8; 32]> {
    ed25519::PublicKey::from_string(address)
        .map(|k| k.0)
        .map_err(|_| "Expected a Stellar G Ed25519 account".into())
}
fn contract_hash(address: &str) -> Result<[u8; 32]> {
    Contract::from_string(address)
        .map(|k| k.0)
        .map_err(|_| "Expected a Stellar C contract".into())
}
fn symbol(value: &str) -> Result<ScVal> {
    Ok(ScVal::Symbol(ScSymbol(
        value.as_bytes().to_vec().try_into().map_err(error)?,
    )))
}
fn bytes(value: &[u8]) -> Result<ScVal> {
    Ok(ScVal::Bytes(ScBytes(
        value.to_vec().try_into().map_err(error)?,
    )))
}
fn vector(value: Vec<ScVal>) -> Result<ScVal> {
    Ok(ScVal::Vec(Some(ScVec(value.try_into().map_err(error)?))))
}
fn map(value: Vec<(&str, ScVal)>) -> Result<ScVal> {
    let entries: Result<Vec<ScMapEntry>> = value
        .into_iter()
        .map(|(key, val)| {
            Ok(ScMapEntry {
                key: symbol(key)?,
                val,
            })
        })
        .collect();
    let mut entries = entries?;
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(ScVal::Map(Some(ScMap(entries.try_into().map_err(error)?))))
}
fn read_xdr<T: ReadXdr>(value: &str) -> Result<T> {
    if value.len() > MAX_BYTES * 2 {
        return Err("XDR input exceeds limit".into());
    }
    T::from_xdr_base64(value, limits()).map_err(error)
}
fn write_xdr<T: WriteXdr>(value: &T) -> Result<String> {
    value.to_xdr_base64(limits()).map_err(error)
}

/// Validate configuration without network calls. Live bindings must also be checked before signing.
pub fn validate_config(c: &Config) -> Result<()> {
    network_passphrase(&c.network)?;
    validate_https_url(&c.rpc_url)?;
    for address in [&c.vault, &c.asset, &c.policy] {
        contract_hash(address)?;
    }
    for address in [&c.owner, &c.executor, &c.authority] {
        account_key(address)?;
    }
    if c.owner == c.executor || c.owner == c.authority || c.executor == c.authority {
        return Err("Owner, executor, and authority must be separate accounts".into());
    }
    if c.vault == c.policy || c.vault == c.asset || c.asset != canonical_asset(&c.network)? {
        return Err("Stellar custody asset/contract binding mismatch".into());
    }
    for digest in [
        &c.vault_wasm_hash,
        &c.policy_wasm_hash,
        &c.policy_source_hash,
    ] {
        if unhex::<32>(digest)? == [0; 32] {
            return Err("Zero pinned contract digest".into());
        }
    }
    Ok(())
}
pub fn validate_https_url(value: &str) -> Result<()> {
    if value.len() > 8192 || value.trim() != value || value.bytes().any(|b| b.is_ascii_control()) {
        return Err("Invalid HTTPS URL".into());
    }
    let url = Url::parse(value).map_err(error)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("HTTPS URL must have no credentials or fragment".into());
    }
    let host = url.host_str().ok_or("HTTPS URL requires a host")?;
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return Err("Local endpoints are not supported".into());
    }
    if let Some(url::Host::Ipv4(ip)) = url.host() {
        if ip.is_private()
            || ip.is_loopback()
            || ip.is_link_local()
            || ip.is_unspecified()
            || ip.is_multicast()
            || ip.is_broadcast()
            || ip.is_documentation()
        {
            return Err("Private/reserved IP endpoint is not supported".into());
        }
    }
    if let Some(url::Host::Ipv6(ip)) = url.host() {
        if ip.is_loopback()
            || ip.is_unspecified()
            || ip.is_multicast()
            || (ip.segments()[0] & 0xfe00) == 0xfc00
            || (ip.segments()[0] & 0xffc0) == 0xfe80
        {
            return Err("Private/reserved IP endpoint is not supported".into());
        }
        if let Some(v4) = ip.to_ipv4_mapped() {
            if v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                return Err("Private IPv4-mapped endpoint".into());
            }
        }
    }
    Ok(())
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing {key}"))
}
fn number(v: &Value, key: &str) -> Result<u64> {
    v.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("Missing integer {key}"))
}
pub fn offer_amount(offer: &Value) -> Result<u64> {
    let s = text(offer, "amount")?;
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return Err("Invalid exact token amount".into());
    }
    let amount = s.parse::<u64>().map_err(error)?;
    if amount == 0 || amount % 10 != 0 {
        return Err("Amount must be positive and exact in six-decimal policy units".into());
    }
    Ok(amount)
}
fn validate_offer(offer: &Value, network: &str, max_token_amount: u64) -> Result<()> {
    network_passphrase(network)?;
    if text(offer, "scheme")? != "exact"
        || text(offer, "network")? != network
        || text(offer, "asset")? != canonical_asset(network)?
        || offer.pointer("/extra/areFeesSponsored") != Some(&Value::Bool(true))
    {
        return Err("Expected sponsored Stellar v2 exact canonical USDC offer".into());
    }
    let to = sc_address(text(offer, "payTo")?)?;
    if to == ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256([0; 32])))) {
        return Err("Zero payment recipient".into());
    }
    if offer_amount(offer)? > max_token_amount {
        return Err("Quote exceeds purchase maximum".into());
    }
    if !(1..=300).contains(&number(offer, "maxTimeoutSeconds")?) {
        return Err("Payment timeout must be between 1 and 300 seconds".into());
    }
    Ok(())
}
pub fn select_offer(required: &Value, network: &str, max_token_amount: u64) -> Result<Value> {
    if required.get("x402Version") != Some(&json!(2)) {
        return Err("Only x402 v2 is supported".into());
    }
    let accepts = required
        .get("accepts")
        .and_then(Value::as_array)
        .ok_or("Missing x402 accepts")?;
    accepts
        .iter()
        .find(|offer| validate_offer(offer, network, max_token_amount).is_ok())
        .cloned()
        .ok_or_else(|| "No supported Stellar exact USDC offer within maximum".into())
}
pub fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    let mut sorted = value.clone();
    sorted.sort_all_objects();
    serde_json::to_vec(&sorted).map_err(error)
}
pub fn request_hash(request: &PaymentRequest) -> Result<[u8; 32]> {
    Ok(sha256(canonical_json(
        &json!({"domain":"allowit_stellar_x402_request_v1","method":request.method,"url":request.url,"bodySha256":hex(&sha256(&request.body)),"resource":request.resource,"paymentRequired":request.payment_required,"selectedOffer":request.selected_offer}),
    )?))
}
fn validate_request(c: &Config, r: &PaymentRequest) -> Result<()> {
    validate_https_url(&r.url)?;
    if !matches!(
        r.method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
    ) || r.body.len() > MAX_BYTES
        || matches!(r.method.as_str(), "GET" | "HEAD") && !r.body.is_empty()
    {
        return Err("Unsupported HTTP method/body".into());
    }
    if r.payment_required.get("x402Version") != Some(&json!(2))
        || r.payment_required.get("resource") != Some(&r.resource)
        || !r
            .payment_required
            .get("accepts")
            .and_then(Value::as_array)
            .ok_or("Missing accepts")?
            .contains(&r.selected_offer)
    {
        return Err("Selected offer/resource differs from original 402".into());
    }
    validate_offer(&r.selected_offer, &c.network, u64::MAX)?;
    let resource_url = text(&r.resource, "url")?;
    validate_https_url(resource_url)?;
    let mut requested = Url::parse(&r.url).map_err(error)?;
    let mut resource = Url::parse(resource_url).map_err(error)?;
    // Some providers publish the endpoint without the request's query string.
    requested.set_query(None);
    resource.set_query(None);
    if requested != resource {
        return Err("402 resource does not match requested endpoint".into());
    }
    if text(&r.selected_offer, "payTo")? == c.vault {
        return Err("Vault cannot pay itself".into());
    }
    Ok(())
}
pub fn validate_state(c: &Config, s: &VaultState) -> Result<()> {
    validate_config(c)?;
    if s.owner != c.owner
        || s.executor != c.executor
        || s.authority != c.authority
        || s.asset != c.asset
        || s.policy != c.policy
        || unhex::<32>(&s.policy_source)? != unhex::<32>(&c.policy_source_hash)?
        || unhex::<32>(&s.policy_wasm)? != unhex::<32>(&c.policy_wasm_hash)?
    {
        return Err("Live vault state differs from pinned configuration".into());
    }
    // Lowering a limit preserves the prior day's spend. The effective-day
    // budget is enforced when preparing a payment, as it is by the contract.
    // Zero is the contract's paused limit. Owner lifecycle reads remain valid;
    // every positive payment still fails the available-budget check.
    Ok(())
}
fn transfer_args(c: &Config, offer: &Value) -> Result<InvokeContractArgs> {
    validate_offer(offer, &c.network, u64::MAX)?;
    Ok(InvokeContractArgs {
        contract_address: sc_address(&c.asset)?,
        function_name: ScSymbol(b"transfer".to_vec().try_into().map_err(error)?),
        args: vec![
            ScVal::Address(sc_address(&c.vault)?),
            ScVal::Address(sc_address(text(offer, "payTo")?)?),
            ScVal::I128(Int128Parts {
                hi: 0,
                lo: offer_amount(offer)?,
            }),
        ]
        .try_into()
        .map_err(error)?,
    })
}
fn envelope(args: InvokeContractArgs) -> Result<TransactionEnvelope> {
    Ok(TransactionEnvelope::Tx(TransactionV1Envelope {
        tx: Transaction {
            source_account: MuxedAccount::Ed25519(Uint256([0; 32])),
            fee: 100,
            seq_num: SequenceNumber(1),
            cond: Preconditions::Time(TimeBounds {
                min_time: TimePoint(0),
                max_time: TimePoint(0),
            }),
            memo: Memo::None,
            operations: vec![Operation {
                source_account: None,
                body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                    host_function: HostFunction::InvokeContract(args),
                    auth: Default::default(),
                }),
            }]
            .try_into()
            .map_err(error)?,
            ext: TransactionExt::V1(SorobanTransactionData::default()),
        },
        signatures: Default::default(),
    }))
}
pub fn transfer_transaction(c: &Config, offer: &Value) -> Result<String> {
    validate_config(c)?;
    write_xdr(&envelope(transfer_args(c, offer)?)?)
}
pub fn state_read_transaction(c: &Config) -> Result<String> {
    validate_config(c)?;
    write_xdr(&envelope(InvokeContractArgs {
        contract_address: sc_address(&c.vault)?,
        function_name: ScSymbol(b"state".to_vec().try_into().map_err(error)?),
        args: Default::default(),
    })?)
}
fn tx_mut(envelope: &mut TransactionEnvelope) -> Result<&mut TransactionV1Envelope> {
    match envelope {
        TransactionEnvelope::Tx(tx) => Ok(tx),
        _ => Err("Expected Stellar v1 transaction envelope".into()),
    }
}
fn op_mut(tx: &mut Transaction) -> Result<&mut InvokeHostFunctionOp> {
    if tx.operations.len() != 1 {
        return Err("Transaction must contain one operation".into());
    }
    let operation = tx.operations.iter_mut().next().ok_or("Missing operation")?;
    if operation.source_account.is_some() {
        return Err("Operation source account is not supported".into());
    }
    match &mut operation.body {
        OperationBody::InvokeHostFunction(op) => Ok(op),
        _ => Err("Expected invokeHostFunction".into()),
    }
}
pub fn attach_simulation_data(transaction: &str, data: &str, min_fee: &str) -> Result<String> {
    let mut envelope: TransactionEnvelope = read_xdr(transaction)?;
    let tx = tx_mut(&mut envelope)?;
    let mut data: SorobanTransactionData = read_xdr(data)?;
    let fee = min_fee.parse::<i64>().map_err(error)?;
    if fee < 0 {
        return Err("Negative Soroban resource fee".into());
    }
    data.resource_fee = fee;
    tx.tx.fee = u32::try_from(fee)
        .map_err(error)?
        .checked_add(100)
        .ok_or("Resource fee overflow")?;
    if tx.tx.fee > MAX_TRANSACTION_FEE_STROOPS {
        return Err("Soroban payment exceeds the facilitator fee ceiling".into());
    }
    tx.tx.ext = TransactionExt::V1(data);
    write_xdr(&envelope)
}
fn credentials(entry: &SorobanAuthorizationEntry) -> Result<&SorobanAddressCredentials> {
    match &entry.credentials {
        SorobanCredentials::Address(c) | SorobanCredentials::AddressV2(c) => Ok(c),
        _ => Err("Only address-based Soroban auth credentials are supported".into()),
    }
}
fn credentials_mut(
    entry: &mut SorobanAuthorizationEntry,
) -> Result<&mut SorobanAddressCredentials> {
    match &mut entry.credentials {
        SorobanCredentials::Address(c) | SorobanCredentials::AddressV2(c) => Ok(c),
        _ => Err("Only address-based Soroban auth credentials are supported".into()),
    }
}
pub fn host_digest(network: &str, authorization_xdr: &str) -> Result<[u8; 32]> {
    let entry: SorobanAuthorizationEntry = read_xdr(authorization_xdr)?;
    let c = credentials(&entry)?;
    let network_id = Hash(sha256(network_passphrase(network)?));
    let preimage = match entry.credentials {
        SorobanCredentials::Address(_) => {
            HashIdPreimage::SorobanAuthorization(HashIdPreimageSorobanAuthorization {
                network_id,
                nonce: c.nonce,
                signature_expiration_ledger: c.signature_expiration_ledger,
                invocation: entry.root_invocation.clone(),
            })
        }
        SorobanCredentials::AddressV2(_) => HashIdPreimage::SorobanAuthorizationWithAddress(
            HashIdPreimageSorobanAuthorizationWithAddress {
                network_id,
                nonce: c.nonce,
                signature_expiration_ledger: c.signature_expiration_ledger,
                address: c.address.clone(),
                invocation: entry.root_invocation.clone(),
            },
        ),
        _ => return Err("Unsupported credential type".into()),
    };
    Ok(sha256(preimage.to_xdr(limits()).map_err(error)?))
}
pub fn authorization_preimage(
    vault: &str,
    host: &[u8; 32],
    nonce: u64,
    revision: u64,
    expires_at: u64,
    request: &[u8; 32],
) -> Result<Vec<u8>> {
    vector(vec![
        symbol("allowit_x402_v1")?,
        ScVal::Address(sc_address(vault)?),
        bytes(host)?,
        ScVal::U64(nonce),
        ScVal::U64(revision),
        ScVal::U64(expires_at),
        bytes(request)?,
    ])?
    .to_xdr(limits())
    .map_err(error)
}
pub fn authorization_digest(
    vault: &str,
    host: &[u8; 32],
    nonce: u64,
    revision: u64,
    expires_at: u64,
    request: &[u8; 32],
) -> Result<[u8; 32]> {
    Ok(sha256(authorization_preimage(
        vault, host, nonce, revision, expires_at, request,
    )?))
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_payment(
    c: &Config,
    s: &VaultState,
    request: PaymentRequest,
    authorization_xdr: &str,
    signature_expiration_ledger: u32,
    expires_at: u64,
    now: u64,
) -> Result<PreparedPayment> {
    validate_state(c, s)?;
    validate_request(c, &request)?;
    let mut entry: SorobanAuthorizationEntry = read_xdr(authorization_xdr)?;
    let creds = credentials_mut(&mut entry)?;
    if creds.address != sc_address(&c.vault)?
        || creds.signature != ScVal::Void
        || signature_expiration_ledger == 0
    {
        return Err("Expected one unsigned vault authorization".into());
    }
    creds.signature_expiration_ledger = signature_expiration_ledger;
    if entry.root_invocation.function
        != SorobanAuthorizedFunction::ContractFn(transfer_args(c, &request.selected_offer)?)
        || !entry.root_invocation.sub_invocations.is_empty()
    {
        return Err("Simulation authorization does not match the exact transfer".into());
    }
    let authorization_xdr = write_xdr(&entry)?;
    let request_hash = request_hash(&request)?;
    let host = host_digest(&c.network, &authorization_xdr)?;
    let digest = authorization_digest(
        &c.vault,
        &host,
        s.nonce,
        s.revision,
        expires_at,
        &request_hash,
    )?;
    let mut tx = envelope(transfer_args(c, &request.selected_offer)?)?;
    op_mut(&mut tx_mut(&mut tx)?.tx)?.auth = vec![entry].try_into().map_err(error)?;
    let p = PreparedPayment {
        config: c.clone(),
        state: s.clone(),
        request,
        transaction_xdr: write_xdr(&tx)?,
        authorization_xdr,
        request_hash: hex(&request_hash),
        host_digest: hex(&host),
        authorization_digest: hex(&digest),
        signature_expiration_ledger,
        expires_at,
        created_at: now,
    };
    validate_prepared(c, &p, now)?;
    Ok(p)
}
pub fn validate_prepared(c: &Config, p: &PreparedPayment, now: u64) -> Result<()> {
    if &p.config != c {
        return Err("Prepared configuration differs from imported config".into());
    }
    validate_state(c, &p.state)?;
    validate_request(c, &p.request)?;
    if !p.state.approved
        || now < p.created_at
        || p.expires_at <= now
        || p.expires_at > p.created_at.checked_add(300).ok_or("Time overflow")?
        || p.created_at / 86400 < p.state.spent_day
    {
        return Err("Payment is unapproved, expired, or has invalid timestamps".into());
    }
    let amount = offer_amount(&p.request.selected_offer)? / 10;
    let spent = if p.created_at / 86400 == p.state.spent_day {
        p.state.spent
    } else {
        0
    };
    if spent.checked_add(amount).ok_or("Budget overflow")? > p.state.daily_limit
        || p.state.nonce == u64::MAX
    {
        return Err("Payment exceeds available policy budget".into());
    }
    let entry: SorobanAuthorizationEntry = read_xdr(&p.authorization_xdr)?;
    let creds = credentials(&entry)?;
    if creds.address != sc_address(&c.vault)?
        || creds.signature != ScVal::Void
        || creds.signature_expiration_ledger != p.signature_expiration_ledger
        || p.signature_expiration_ledger == 0
        || entry.root_invocation.function
            != SorobanAuthorizedFunction::ContractFn(transfer_args(c, &p.request.selected_offer)?)
        || !entry.root_invocation.sub_invocations.is_empty()
    {
        return Err("Prepared authorization mismatch".into());
    }
    let mut envelope: TransactionEnvelope = read_xdr(&p.transaction_xdr)?;
    let tx = tx_mut(&mut envelope)?;
    if !(100..=MAX_TRANSACTION_FEE_STROOPS).contains(&tx.tx.fee) {
        return Err("Payment transaction fee is outside facilitator limits".into());
    }
    match &tx.tx.ext {
        TransactionExt::V1(data)
            if data.resource_fee >= 0 && data.resource_fee <= i64::from(tx.tx.fee - 100) => {}
        _ => return Err("Payment Soroban resources/fee are invalid".into()),
    }
    if !tx.signatures.is_empty()
        || tx.tx.source_account != MuxedAccount::Ed25519(Uint256([0; 32]))
        || tx.tx.seq_num != SequenceNumber(1)
        || tx.tx.memo != Memo::None
        || tx.tx.cond
            != Preconditions::Time(TimeBounds {
                min_time: TimePoint(0),
                max_time: TimePoint(0),
            })
    {
        return Err("Unexpected prepared transaction envelope".into());
    }
    let op = op_mut(&mut tx.tx)?;
    if op.host_function
        != HostFunction::InvokeContract(transfer_args(c, &p.request.selected_offer)?)
        || op.auth.as_slice() != [entry]
    {
        return Err("Prepared token transfer/auth mismatch".into());
    }
    let req = request_hash(&p.request)?;
    let host = host_digest(&c.network, &p.authorization_xdr)?;
    let digest = authorization_digest(
        &c.vault,
        &host,
        p.state.nonce,
        p.state.revision,
        p.expires_at,
        &req,
    )?;
    if unhex::<32>(&p.request_hash)? != req
        || unhex::<32>(&p.host_digest)? != host
        || unhex::<32>(&p.authorization_digest)? != digest
    {
        return Err("Prepared cryptographic digest mismatch".into());
    }
    Ok(())
}
pub fn verify_signature(address: &str, digest: &[u8; 32], signature: &[u8; 64]) -> Result<()> {
    VerifyingKey::from_bytes(&account_key(address)?)
        .map_err(error)?
        .verify_strict(digest, &Signature::from_bytes(signature))
        .map_err(|_| "Invalid Ed25519 payment signature".into())
}
pub fn authority_sign(
    c: &Config,
    p: &PreparedPayment,
    seed: &[u8; 32],
) -> Result<DetachedApproval> {
    validate_prepared(c, p, p.created_at)?;
    if account_for_seed(seed) != c.authority {
        return Err("Authority seed does not match configured signer".into());
    }
    let digest = unhex::<32>(&p.authorization_digest)?;
    Ok(DetachedApproval {
        authority: c.authority.clone(),
        digest: hex(&digest),
        signature: hex(&SigningKey::from_bytes(seed).sign(&digest).to_bytes()),
    })
}
pub fn verify_approval(c: &Config, p: &PreparedPayment, approval: &DetachedApproval) -> Result<()> {
    validate_prepared(c, p, p.created_at)?;
    if approval.authority != c.authority
        || unhex::<32>(&approval.digest)? != unhex::<32>(&p.authorization_digest)?
    {
        return Err("Detached approval is bound to another payment".into());
    }
    verify_signature(
        &c.authority,
        &unhex::<32>(&approval.digest)?,
        &unhex::<64>(&approval.signature)?,
    )
}
fn authorization_value(
    p: &PreparedPayment,
    executor: &[u8; 64],
    authority: &[u8; 64],
) -> Result<ScVal> {
    map(vec![
        ("authority_signature", bytes(authority)?),
        ("executor_signature", bytes(executor)?),
        ("expires_at", ScVal::U64(p.expires_at)),
        ("nonce", ScVal::U64(p.state.nonce)),
        ("request_hash", bytes(&unhex::<32>(&p.request_hash)?)?),
        ("revision", ScVal::U64(p.state.revision)),
    ])
}
pub fn sign_payment(
    c: &Config,
    p: &PreparedPayment,
    executor_seed: &[u8; 32],
    approval: &DetachedApproval,
) -> Result<SignedPayment> {
    verify_approval(c, p, approval)?;
    if account_for_seed(executor_seed) != c.executor {
        return Err("Executor seed does not match configured signer".into());
    }
    let executor = SigningKey::from_bytes(executor_seed)
        .sign(&unhex::<32>(&p.authorization_digest)?)
        .to_bytes();
    let authority = unhex::<64>(&approval.signature)?;
    let mut tx: TransactionEnvelope = read_xdr(&p.transaction_xdr)?;
    let op = op_mut(&mut tx_mut(&mut tx)?.tx)?;
    credentials_mut(op.auth.iter_mut().next().ok_or("Missing authorization")?)?.signature =
        authorization_value(p, &executor, &authority)?;
    let transaction_xdr = write_xdr(&tx)?;
    let payload = json!({"x402Version":2,"resource":p.request.resource,"accepted":p.request.selected_offer,"payload":{"transaction":transaction_xdr}});
    let payment_signature = STANDARD.encode(serde_json::to_vec(&payload).map_err(error)?);
    let signed = SignedPayment {
        payload,
        payment_signature,
        transaction_xdr,
    };
    validate_signed(c, p, &signed)?;
    Ok(signed)
}
pub fn validate_signed(c: &Config, p: &PreparedPayment, signed: &SignedPayment) -> Result<()> {
    validate_prepared(c, p, p.created_at)?;
    if signed.payload.get("x402Version") != Some(&json!(2))
        || signed.payload.get("resource") != Some(&p.request.resource)
        || signed.payload.get("accepted") != Some(&p.request.selected_offer)
        || signed
            .payload
            .pointer("/payload/transaction")
            .and_then(Value::as_str)
            != Some(&signed.transaction_xdr)
        || signed.payload.as_object().map(|v| v.len()) != Some(4)
        || signed
            .payload
            .get("payload")
            .and_then(Value::as_object)
            .map(|v| v.len())
            != Some(1)
    {
        return Err("Signed x402 payload differs from prepared payment".into());
    }
    let decoded: Value =
        serde_json::from_slice(&STANDARD.decode(&signed.payment_signature).map_err(error)?)
            .map_err(error)?;
    if decoded != signed.payload {
        return Err("PAYMENT-SIGNATURE header differs from saved credential".into());
    }
    let mut envelope: TransactionEnvelope = read_xdr(&signed.transaction_xdr)?;
    let tx = tx_mut(&mut envelope)?;
    let op = op_mut(&mut tx.tx)?;
    if op.auth.len() != 1 {
        return Err("Unexpected signed authorization count".into());
    }
    let signature = credentials(&op.auth[0])?.signature.clone();
    let fields = sc_map(&signature)?;
    if fields.len() != 6 {
        return Err("Invalid payment authorization fields".into());
    }
    let executor = sc_bytes::<64>(
        fields
            .get("executor_signature")
            .ok_or("Missing executor signature")?,
    )?;
    let authority = sc_bytes::<64>(
        fields
            .get("authority_signature")
            .ok_or("Missing authority signature")?,
    )?;
    if signature != authorization_value(p, &executor, &authority)? {
        return Err("Payment authorization metadata mismatch".into());
    }
    verify_signature(
        &c.executor,
        &unhex::<32>(&p.authorization_digest)?,
        &executor,
    )?;
    verify_signature(
        &c.authority,
        &unhex::<32>(&p.authorization_digest)?,
        &authority,
    )?;
    credentials_mut(op.auth.iter_mut().next().ok_or("Missing authorization")?)?.signature =
        ScVal::Void;
    if write_xdr(&envelope)? != p.transaction_xdr {
        return Err("Signed envelope differs from prepared envelope".into());
    }
    Ok(())
}
fn sc_map(value: &ScVal) -> Result<BTreeMap<String, ScVal>> {
    let entries = match value {
        ScVal::Map(Some(map)) => &map.0,
        _ => return Err("Expected Soroban map".into()),
    };
    let mut out = BTreeMap::new();
    let mut previous: Option<&ScVal> = None;
    for entry in entries {
        if previous.is_some_and(|p| p >= &entry.key) {
            return Err("Unsorted or duplicate Soroban map key".into());
        }
        previous = Some(&entry.key);
        let key = match &entry.key {
            ScVal::Symbol(s) => String::from_utf8(s.0.as_slice().to_vec()).map_err(error)?,
            _ => return Err("Expected symbol map key".into()),
        };
        if out.insert(key, entry.val.clone()).is_some() {
            return Err("Duplicate Soroban map key".into());
        }
    }
    Ok(out)
}
fn sc_bytes<const N: usize>(value: &ScVal) -> Result<[u8; N]> {
    match value {
        ScVal::Bytes(b) => {
            b.0.as_slice()
                .try_into()
                .map_err(|_| "Invalid fixed-length Soroban bytes".into())
        }
        _ => Err("Expected Soroban bytes".into()),
    }
}
fn sc_u64(value: &ScVal) -> Result<u64> {
    match value {
        ScVal::U64(v) => Ok(*v),
        _ => Err("Expected Soroban u64".into()),
    }
}
pub fn parse_vault_state(result_xdr: &str) -> Result<VaultState> {
    let value: ScVal = read_xdr(result_xdr)?;
    let map = sc_map(&value)?;
    if map.len() != 13 {
        return Err("Unexpected vault state schema".into());
    }
    let get = |key: &str| {
        map.get(key)
            .ok_or_else(|| format!("Missing vault field {key}"))
    };
    let address = |key: &str| match get(key)? {
        ScVal::Address(a) => address_text(a),
        _ => Err("Expected vault address".into()),
    };
    let approved = match get("approved")? {
        ScVal::Bool(v) => *v,
        _ => return Err("Expected approved boolean".into()),
    };
    Ok(VaultState {
        owner: address("owner")?,
        executor: address("executor")?,
        authority: address("authority")?,
        asset: address("asset")?,
        policy: address("policy")?,
        policy_source: hex(&sc_bytes::<32>(get("policy_source")?)?),
        policy_wasm: hex(&sc_bytes::<32>(get("policy_wasm")?)?),
        daily_limit: sc_u64(get("daily_limit")?)?,
        spent: sc_u64(get("spent")?)?,
        spent_day: sc_u64(get("spent_day")?)?,
        nonce: sc_u64(get("nonce")?)?,
        revision: sc_u64(get("revision")?)?,
        approved,
    })
}

fn instance_key(address: &str) -> Result<LedgerKey> {
    Ok(LedgerKey::ContractData(LedgerKeyContractData {
        contract: sc_address(address)?,
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    }))
}
/// Keys for the vault and policy instances and their exact pinned WASM bytes.
pub fn ledger_keys(c: &Config) -> Result<Vec<String>> {
    validate_config(c)?;
    let keys = [
        instance_key(&c.vault)?,
        instance_key(&c.policy)?,
        LedgerKey::ContractCode(LedgerKeyContractCode {
            hash: Hash(unhex::<32>(&c.vault_wasm_hash)?),
        }),
        LedgerKey::ContractCode(LedgerKeyContractCode {
            hash: Hash(unhex::<32>(&c.policy_wasm_hash)?),
        }),
    ];
    keys.iter().map(write_xdr).collect()
}
/// Verify getLedgerEntries against pinned WASM and addresses before either signer is accessed.
pub fn validate_contract_entries(c: &Config, entries: &[Value]) -> Result<()> {
    let expected = ledger_keys(c)?;
    if entries.len() != expected.len() {
        return Err("Pinned contract instance/code entry is missing or archived".into());
    }
    let mut seen = BTreeMap::new();
    for entry in entries {
        let key = text(entry, "key")?;
        if !expected.iter().any(|e| e == key) || seen.insert(key, ()).is_some() {
            return Err("Unexpected or duplicate pinned ledger entry".into());
        }
        let data: LedgerEntryData = read_xdr(text(entry, "xdr")?)?;
        if write_xdr(&data.to_key())? != key {
            return Err("Ledger entry key does not match returned data".into());
        }
        match data {
            LedgerEntryData::ContractData(data) => {
                let address = address_text(&data.contract)?;
                let expected = if address == c.vault {
                    &c.vault_wasm_hash
                } else if address == c.policy {
                    &c.policy_wasm_hash
                } else {
                    return Err("Unexpected instance contract".into());
                };
                match data.val {
                    ScVal::ContractInstance(instance)
                        if instance.executable
                            == ContractExecutable::Wasm(Hash(unhex::<32>(expected)?)) => {}
                    _ => return Err("Pinned contract WASM differs from live executable".into()),
                }
            }
            LedgerEntryData::ContractCode(code) => {
                if code.hash.0 != sha256(code.code.as_slice()) {
                    return Err("Pinned contract code digest mismatch".into());
                }
            }
            _ => return Err("Expected pinned contract data/code".into()),
        }
    }
    Ok(())
}
pub fn parse_payment_response(header: &str, c: &Config) -> Result<SettlementReceipt> {
    if header.len() > 65536 {
        return Err("Payment receipt exceeds limit".into());
    }
    let value: Value =
        serde_json::from_slice(&STANDARD.decode(header).map_err(error)?).map_err(error)?;
    if value.get("success") != Some(&Value::Bool(true)) {
        return Err("Facilitator did not report a successful settlement".into());
    }
    let receipt = SettlementReceipt {
        transaction: text(&value, "transaction")?.into(),
        payer: text(&value, "payer")?.into(),
        network: text(&value, "network")?.into(),
    };
    validate_receipt_identity(c, &receipt)?;
    Ok(receipt)
}
fn validate_receipt_identity(c: &Config, receipt: &SettlementReceipt) -> Result<()> {
    validate_config(c)?;
    if receipt.payer != c.vault
        || receipt.network != c.network
        || unhex::<32>(&receipt.transaction)? == [0; 32]
    {
        return Err("Settlement receipt belongs to another payer/network".into());
    }
    Ok(())
}
fn transaction(envelope: &TransactionEnvelope) -> Result<&Transaction> {
    match envelope {
        TransactionEnvelope::Tx(tx) => Ok(&tx.tx),
        TransactionEnvelope::TxFeeBump(tx) => match &tx.tx.inner_tx {
            FeeBumpTransactionInnerTx::Tx(inner) => Ok(&inner.tx),
        },
        _ => Err("Unsupported confirmed transaction envelope".into()),
    }
}
/// Confirm the facilitator's rewritten transaction, not the hash of the client's unsigned envelope.
/// Receipt validation intentionally works after authorization expiry for safe recovery.
pub fn validate_receipt(
    c: &Config,
    p: &PreparedPayment,
    signed: &SignedPayment,
    receipt: &SettlementReceipt,
    rpc: &Value,
) -> Result<()> {
    validate_signed(c, p, signed)?;
    validate_receipt_identity(c, receipt)?;
    if rpc.get("status").and_then(Value::as_str) != Some("SUCCESS") {
        return Err("Stellar transaction is not confirmed successful".into());
    }
    if let Some(hash) = rpc.get("txHash").and_then(Value::as_str) {
        if unhex::<32>(hash)? != unhex::<32>(&receipt.transaction)? {
            return Err("RPC transaction hash differs from settlement receipt".into());
        }
    }
    let confirmed: TransactionEnvelope = read_xdr(text(rpc, "envelopeXdr")?)?;
    if confirmed
        .hash(sha256(network_passphrase(&c.network)?))
        .map_err(error)?
        != unhex::<32>(&receipt.transaction)?
    {
        return Err("Confirmed envelope hash/network differs from receipt".into());
    }
    let submitted: TransactionEnvelope = read_xdr(&signed.transaction_xdr)?;
    let confirmed_tx = transaction(&confirmed)?;
    if confirmed_tx.operations.as_slice() != transaction(&submitted)?.operations.as_slice() {
        return Err("Confirmed transfer or authorization differs from signed purchase".into());
    }
    let result: TransactionResult = read_xdr(text(rpc, "resultXdr")?)?;
    match result.result {
        TransactionResultResult::TxSuccess(_) => {}
        TransactionResultResult::TxFeeBumpInnerSuccess(pair) => match pair.result.result {
            InnerTransactionResultResult::TxSuccess(_) => {}
            _ => return Err("Fee-bumped inner transaction did not succeed".into()),
        },
        _ => return Err("On-chain result was not successful".into()),
    }
    let meta: TransactionMeta = read_xdr(text(rpc, "resultMetaXdr")?)?;
    let events: Vec<ContractEvent> = match meta {
        TransactionMeta::V3(meta) => meta
            .soroban_meta
            .ok_or("Missing Soroban confirmation metadata")?
            .events
            .to_vec(),
        TransactionMeta::V4(meta) => {
            if meta.operations.len() != 1 {
                return Err("Unexpected confirmed operation metadata".into());
            }
            meta.operations[0].events.to_vec()
        }
        _ => return Err("Expected Soroban transaction metadata".into()),
    };
    let contract_events: Vec<&ContractEvent> = events
        .iter()
        .filter(|e| e.type_ == ContractEventType::Contract)
        .collect();
    if contract_events.len() != 1 {
        return Err("Expected one confirmed token transfer event".into());
    }
    let event = contract_events[0];
    if event.contract_id.as_ref() != Some(&ContractId(Hash(contract_hash(&c.asset)?))) {
        return Err("Transfer event asset mismatch".into());
    }
    let ContractEventBody::V0(body) = &event.body;
    if body.topics.len() < 3
        || body.topics.len() > 4
        || body.topics[0] != symbol("transfer")?
        || body.topics[1] != ScVal::Address(sc_address(&c.vault)?)
        || body.topics[2] != ScVal::Address(sc_address(text(&p.request.selected_offer, "payTo")?)?)
        || body.data
            != ScVal::I128(Int128Parts {
                hi: 0,
                lo: offer_amount(&p.request.selected_offer)?,
            })
    {
        return Err("Confirmed token transfer event does not match purchase".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
