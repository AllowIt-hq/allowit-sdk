//! Stellar adapters for the existing typed execution request and operation lifecycle.
//! Native and ODC custody use the same authored policy and exact operation identity.
use allowit_policy_sdk::typed_workflow::{
    AbiValue, AccountId as WorkflowAccount, AccountKind, Amount256, Chain, ExecutionInput,
    ExecutionRequest, NativeInstruction, WorkflowAsset,
};
use ed25519_dalek::{Signature as EdSignature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stellar_strkey::{Contract, Strkey, ed25519};
use stellar_xdr::*;

pub mod rpc;
pub type Result<T> = std::result::Result<T, String>;
pub const PROFILE: &str = "allowit-stellar-wallet-v1";
pub const ARGUMENT_SCHEMA: &[u8] = b"allowit.stellar.sep41_transfer.v1:ScValMap(amount:i128,from:Address,to:Address,token:Address)";
const MAX_BYTES: usize = 1_048_576;

pub fn hash(bytes: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(bytes.as_ref()).into()
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex<const N: usize>(s: &str) -> Result<[u8; N]> {
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid hexadecimal identity".into());
    }
    let mut out = [0; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(err)?;
    }
    Ok(out)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn limits() -> Limits {
    Limits {
        depth: 64,
        len: MAX_BYTES,
    }
}
fn xdr<T: WriteXdr>(value: &T) -> Result<String> {
    value.to_xdr_base64(limits()).map_err(err)
}
fn read<T: ReadXdr>(value: &str) -> Result<T> {
    if value.len() > MAX_BYTES * 2 {
        return Err("XDR exceeds limit".into());
    }
    T::from_xdr_base64(value, limits()).map_err(err)
}
pub fn passphrase(network: &str) -> Result<&'static str> {
    match network {
        "stellar:mainnet" => Ok("Public Global Stellar Network ; September 2015"),
        "stellar:testnet" => Ok("Test SDF Network ; September 2015"),
        _ => Err("Unsupported Stellar network".into()),
    }
}
pub fn network_id(network: &str) -> Result<[u8; 32]> {
    Ok(hash(passphrase(network)?))
}
/// Decode a canonical local Ed25519 seed. Never include this value in an export bundle.
pub fn decode_seed(value: &str) -> Result<[u8; 32]> {
    ed25519::PrivateKey::from_string(value)
        .map(|v| v.0)
        .map_err(|_| "Expected a canonical Stellar S seed".into())
}
pub fn account_for_seed(seed: &[u8; 32]) -> String {
    ed25519::PublicKey(SigningKey::from_bytes(seed).verifying_key().to_bytes()).to_string()
}
pub fn account_key(address: &str) -> Result<[u8; 32]> {
    ed25519::PublicKey::from_string(address)
        .map(|v| v.0)
        .map_err(|_| "Expected canonical Stellar G account".into())
}
pub fn address(value: &str) -> Result<ScAddress> {
    match Strkey::from_string(value).map_err(err)? {
        Strkey::PublicKeyEd25519(v) => Ok(ScAddress::Account(AccountId(
            PublicKey::PublicKeyTypeEd25519(Uint256(v.0)),
        ))),
        Strkey::Contract(v) => Ok(ScAddress::Contract(ContractId(Hash(v.0)))),
        _ => Err("Only canonical G and C addresses are admitted".into()),
    }
}
pub fn address_text(value: &ScAddress) -> Result<String> {
    match value {
        ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(v))) => {
            Ok(ed25519::PublicKey(v.0).to_string())
        }
        ScAddress::Contract(v) => Ok(Contract(v.0.0).to_string()),
        _ => Err("Unsupported address".into()),
    }
}
fn workflow_address(value: &WorkflowAccount) -> Result<String> {
    if value.chain != Chain::Stellar {
        return Err("Wrong request rail".into());
    }
    match value.kind {
        AccountKind::Wallet => Ok(ed25519::PublicKey(value.address).to_string()),
        AccountKind::Contract => Ok(Contract(value.address).to_string()),
        _ => Err("Unsupported Stellar account kind".into()),
    }
}
fn value_symbol(v: &str) -> Result<ScVal> {
    Ok(ScVal::Symbol(ScSymbol(
        v.as_bytes().to_vec().try_into().map_err(err)?,
    )))
}
fn value_bytes(v: &[u8]) -> Result<ScVal> {
    Ok(ScVal::Bytes(ScBytes(v.to_vec().try_into().map_err(err)?)))
}
fn value_map(entries: Vec<(&str, ScVal)>) -> Result<ScVal> {
    let mut out = entries
        .into_iter()
        .map(|(k, val)| {
            Ok(ScMapEntry {
                key: value_symbol(k)?,
                val,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(ScVal::Map(Some(ScMap(out.try_into().map_err(err)?))))
}
fn i128_value(v: i128) -> ScVal {
    ScVal::I128(Int128Parts {
        hi: (v >> 64) as i64,
        lo: v as u64,
    })
}
fn amount(v: &Amount256) -> Result<i128> {
    if v.0[..16].iter().any(|b| *b != 0) {
        return Err("Amount exceeds native i128".into());
    }
    i128::try_from(u128::from_be_bytes(v.0[16..].try_into().unwrap())).map_err(err)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum WalletRoute {
    Native,
    Odc,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WalletConfig {
    pub network: String,
    pub policy_instance: [u8; 32],
    pub wallet: String,
    pub owner: String,
    pub agent: String,
    pub engine: String,
    pub route: WalletRoute,
    pub asset: String,
    pub asset_symbol: String,
    #[serde(default)]
    pub asset_issuer: Option<String>,
    pub policy_source: String,
    pub source_hash: [u8; 32],
    pub module_digest: [u8; 32],
    pub max_fee_stroops: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountSigner {
    pub key: String,
    pub weight: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountState {
    pub address: String,
    pub sequence: i64,
    pub master_weight: u32,
    pub low_threshold: u32,
    pub medium_threshold: u32,
    pub high_threshold: u32,
    pub signers: Vec<AccountSigner>,
}
impl AccountState {
    pub fn validate_wallet(&self, c: &WalletConfig) -> Result<()> {
        if self.address != c.wallet
            || self.sequence < 0
            || (
                self.low_threshold,
                self.medium_threshold,
                self.high_threshold,
            ) != (2, 2, 3)
        {
            return Err("Native wallet thresholds or identity differ".into());
        }
        let mut expected = std::collections::BTreeMap::new();
        expected.insert(c.agent.clone(), 1);
        expected.insert(c.engine.clone(), 1);
        let master = if c.owner == c.wallet {
            3
        } else {
            expected.insert(c.owner.clone(), 3);
            0
        };
        if self.master_weight != master {
            return Err("Native wallet master weight differs".into());
        }
        let mut actual = std::collections::BTreeMap::new();
        for s in &self.signers {
            account_key(&s.key)?;
            if s.key == self.address
                || s.weight == 0
                || actual.insert(s.key.clone(), s.weight).is_some()
            {
                return Err("Duplicate or unexpected native signer".into());
            }
        }
        if actual != expected {
            return Err("Native wallet must contain exactly owner, agent and engine".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedState {
    pub source: AccountState,
    pub nonce: u64,
    pub revision: u64,
    pub spent_daily: i128,
    pub spent_total: i128,
    pub spent_day: u64,
    pub calls: u64,
    pub ledger: u32,
    pub module_digest: [u8; 32],
    pub source_hash: [u8; 32],
    pub owner: String,
    pub agent: String,
    pub engine: String,
    pub policy_instance: [u8; 32],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedOperation {
    pub version: u32,
    pub config: WalletConfig,
    pub request: ExecutionRequest,
    pub state: TrustedState,
    pub transaction_xdr: String,
    pub transaction_hash: [u8; 32],
    pub request_hash: [u8; 32],
    pub simulation_data: Option<String>,
    pub created_at: u64,
    pub expires_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetachedSignature {
    pub signer: String,
    pub signature: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedOperation {
    pub prepared: PreparedOperation,
    pub transaction_xdr: String,
    pub transaction_hash: [u8; 32],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum OperationStatus {
    Prepared,
    Submitted,
    SubmissionUnknown,
    Confirmed,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub token: String,
    pub source: String,
    pub recipient: String,
    pub amount: i128,
    pub native: bool,
}

pub fn validate_config(c: &WalletConfig) -> Result<()> {
    passphrase(&c.network)?;
    if c.policy_instance == [0; 32]
        || c.source_hash != hash(c.policy_source.as_bytes())
        || c.max_fee_stroops < 100
        || c.max_fee_stroops
            > match c.route {
                WalletRoute::Native => 1_000_000,
                WalletRoute::Odc => 2_000_000,
            }
    {
        return Err("Invalid installed policy or fee binding".into());
    }
    for actor in [&c.owner, &c.agent] {
        account_key(actor)?;
    }
    if c.owner == c.agent || c.owner == c.engine || c.agent == c.engine {
        return Err("Wallet roles must differ".into());
    }
    match c.route {
        WalletRoute::Native => {
            account_key(&c.wallet)?;
            account_key(&c.engine)?;
        }
        WalletRoute::Odc => {
            Contract::from_string(&c.engine).map_err(err)?;
            Contract::from_string(&c.wallet).map_err(err)?;
            if c.module_digest == [0; 32] {
                return Err("Missing installed ODC code identity".into());
            }
        }
    }
    address(&c.asset)?;
    if c.asset != sac_address(&c.network, &asset_xdr(c)?)? {
        return Err("SAC does not match installed native asset identity".into());
    }
    Ok(())
}
/// Decode only the admitted catalog transfer profile, with exact effect and fee bounds.
pub fn transfer_from_request(c: &WalletConfig, r: &ExecutionRequest) -> Result<Transfer> {
    validate_config(c)?;
    if r.operation.policy_instance != c.policy_instance || r.operation.nonce == [0; 32] {
        return Err("Operation installation or identity differs".into());
    }
    let ExecutionInput::Instructions(instructions) = &r.input else {
        return Err("Stellar execution requires typed instructions".into());
    };
    if instructions.len() != 1 || r.effect_bounds.len() != 1 || !r.evidence.is_empty() {
        return Err("Only one admitted transfer without unresolved evidence is supported".into());
    }
    let transfer = match &instructions[0] {
        NativeInstruction::StellarNativeTransfer {
            source,
            destination,
            stroops,
        } => Transfer {
            token: c.asset.clone(),
            source: workflow_address(source)?,
            recipient: workflow_address(destination)?,
            amount: i128::from(*stroops),
            native: true,
        },
        NativeInstruction::StellarCall {
            contract,
            schema_digest,
            function,
            arguments,
        } => {
            if workflow_address(contract)? != c.asset
                || *schema_digest != hash(ARGUMENT_SCHEMA)
                || function != "transfer"
                || arguments.len() != 3
            {
                return Err("Unknown Stellar module, schema or method".into());
            }
            let [
                AbiValue::Account(from),
                AbiValue::Account(to),
                AbiValue::Signed128(units),
            ] = arguments.as_slice()
            else {
                return Err("Invalid SEP-41 arguments".into());
            };
            Transfer {
                token: c.asset.clone(),
                source: workflow_address(from)?,
                recipient: workflow_address(to)?,
                amount: *units,
                native: false,
            }
        }
        _ => return Err("Unsupported Stellar instruction".into()),
    };
    if transfer.native && asset_xdr(c)? != Asset::Native {
        return Err("Native XLM request differs from installed asset".into());
    }
    if transfer.amount <= 0 || transfer.source != c.wallet || transfer.recipient == c.wallet {
        return Err("Invalid transfer source, amount or recipient".into());
    }
    let effect = &r.effect_bounds[0];
    let asset = match &effect.asset {
        WorkflowAsset::Native { chain, network } => {
            *chain == Chain::Stellar && *network == network_id(&c.network)? && transfer.native
        }
        WorkflowAsset::Token(a) => {
            a.network == network_id(&c.network)?
                && workflow_address(&a.token)? == c.asset
                && !transfer.native
        }
    };
    if !asset
        || workflow_address(&effect.source)? != transfer.source
        || workflow_address(&effect.beneficiary)? != transfer.recipient
        || amount(&effect.max_debit)? != transfer.amount
        || amount(&effect.min_credit)? != transfer.amount
        || amount(&effect.max_burn)? != 0
    {
        return Err("Transfer effects differ from approved bounds".into());
    }
    let fees = r
        .fee_bounds
        .as_ref()
        .ok_or("Resolved native fee bounds are required")?;
    if fees.len() != 1 {
        return Err("One native fee bound is required".into());
    }
    let fee = &fees[0];
    let payer = if c.route == WalletRoute::Native {
        &c.wallet
    } else {
        &c.agent
    };
    if workflow_address(&fee.payer)? != *payer
        || !matches!(&fee.asset,WorkflowAsset::Native{chain:Chain::Stellar,network} if *network==network_id(&c.network)?)
        || amount(&fee.max_fee)? < i128::from(c.max_fee_stroops)
    {
        return Err("Native fee payer or fee bound differs".into());
    }
    Ok(transfer)
}
pub fn asset_xdr(c: &WalletConfig) -> Result<Asset> {
    if c.asset_symbol == "XLM" && c.asset_issuer.is_none() {
        return Ok(Asset::Native);
    }
    let issuer = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(account_key(
        c.asset_issuer
            .as_ref()
            .ok_or("Issued asset needs exact issuer")?,
    )?)));
    let code = c.asset_symbol.as_bytes();
    if code.is_empty() || code.len() > 12 || !code.iter().all(|v| v.is_ascii_alphanumeric()) {
        return Err("Invalid issued asset code".into());
    }
    if code.len() <= 4 {
        let mut v = [0; 4];
        v[..code.len()].copy_from_slice(code);
        Ok(Asset::CreditAlphanum4(AlphaNum4 {
            asset_code: AssetCode4(v),
            issuer,
        }))
    } else {
        let mut v = [0; 12];
        v[..code.len()].copy_from_slice(code);
        Ok(Asset::CreditAlphanum12(AlphaNum12 {
            asset_code: AssetCode12(v),
            issuer,
        }))
    }
}
pub fn sac_address(network: &str, asset: &Asset) -> Result<String> {
    let preimage = HashIdPreimage::ContractId(HashIdPreimageContractId {
        network_id: Hash(network_id(network)?),
        contract_id_preimage: ContractIdPreimage::Asset(asset.clone()),
    });
    Ok(Contract(hash(preimage.to_xdr(limits()).map_err(err)?)).to_string())
}
pub fn verify_policy(
    c: &WalletConfig,
    r: &ExecutionRequest,
    state: &TrustedState,
    now: u64,
) -> Result<()> {
    let t = transfer_from_request(c, r)?;
    let policy = allowit_policy_sdk::compile_stellar_policy(&c.policy_source).map_err(err)?;
    if policy.source_hash != c.source_hash
        || !policy.evaluate(&allowit_policy_sdk::StellarPolicyContext {
            asset: c.asset.clone(),
            recipient: t.recipient,
            token: c.asset_symbol.clone(),
            decimals: 7,
            amount: t.amount,
            spent_daily: state.spent_daily,
            spent_total: state.spent_total,
            calls: state.calls,
            now,
            spent_day: state.spent_day,
        })
    {
        return Err("Authored policy rejected this exact Stellar operation".into());
    }
    Ok(())
}
fn muxed(value: &str) -> Result<MuxedAccount> {
    Ok(MuxedAccount::Ed25519(Uint256(account_key(value)?)))
}
fn transaction_hash(network: &str, tx: &Transaction) -> Result<[u8; 32]> {
    Ok(hash(
        TransactionSignaturePayload {
            network_id: Hash(network_id(network)?),
            tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(tx.clone()),
        }
        .to_xdr(limits())
        .map_err(err)?,
    ))
}
fn transfer_args(c: &WalletConfig, t: &Transfer) -> Result<ScVal> {
    value_map(vec![
        ("amount", i128_value(t.amount)),
        ("from", ScVal::Address(address(&c.wallet)?)),
        ("to", ScVal::Address(address(&t.recipient)?)),
        ("token", ScVal::Address(address(&c.asset)?)),
    ])
}
fn odc_request(
    c: &WalletConfig,
    r: &ExecutionRequest,
    state: &TrustedState,
    t: &Transfer,
    expires: u64,
) -> Result<ScVal> {
    let args = transfer_args(c, t)?.to_xdr(limits()).map_err(err)?;
    let invocation = value_map(vec![
        ("module_digest", value_bytes(&c.module_digest)?),
        ("entrypoint", value_symbol("_execute")?),
        (
            "argument_schema_digest",
            value_bytes(&hash(ARGUMENT_SCHEMA))?,
        ),
        ("arguments", value_bytes(&args)?),
    ])?;
    value_map(vec![
        ("policy_instance", value_bytes(&c.policy_instance)?),
        ("operation_id", value_bytes(&r.operation.nonce)?),
        ("nonce", ScVal::U64(state.nonce)),
        ("revision", ScVal::U64(state.revision)),
        ("expires_at", ScVal::U64(expires)),
        ("invocation", invocation),
    ])
}
fn build(
    c: &WalletConfig,
    r: &ExecutionRequest,
    state: &TrustedState,
    now: u64,
    expires: u64,
    retained_native_lower_bound: bool,
) -> Result<Transaction> {
    let t = transfer_from_request(c, r)?;
    verify_policy(c, r, state, now)?;
    if c.route == WalletRoute::Native
        && expires
            > (now / 86400)
                .checked_add(1)
                .and_then(|day| day.checked_mul(86400))
                .ok_or("UTC boundary overflow")?
    {
        return Err("Native approval cannot cross its UTC budget day".into());
    }
    if expires <= now || expires > now.checked_add(300).ok_or("Expiry overflow")? {
        return Err("Operation expiry must be within 300 seconds".into());
    }
    if state.source_hash != c.source_hash {
        return Err("Installed authored source identity differs".into());
    }
    if state.policy_instance != c.policy_instance
        || state.owner != c.owner
        || state.agent != c.agent
        || state.engine != c.engine
    {
        return Err("Trusted wallet identity differs".into());
    }
    let body = match c.route {
        WalletRoute::Native => {
            state.source.validate_wallet(c)?;
            OperationBody::Payment(PaymentOp {
                destination: muxed(&t.recipient)?,
                asset: asset_xdr(c)?,
                amount: i64::try_from(t.amount).map_err(|_| "Classic payment exceeds i64")?,
            })
        }
        WalletRoute::Odc => {
            if state.source.master_weight != 1
                || !state.source.signers.is_empty()
                || state.source.sequence < 0
                || state.source.low_threshold > 1
                || state.source.medium_threshold > 1
                || state.source.high_threshold > 1
            {
                return Err("ODC agent transaction account is not owner-only".into());
            }
            if state.module_digest != c.module_digest || state.source.address != c.agent {
                return Err("ODC executable or transaction source differs".into());
            }
            OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: HostFunction::InvokeContract(InvokeContractArgs {
                    contract_address: address(&c.wallet)?,
                    function_name: ScSymbol(b"execute".to_vec().try_into().map_err(err)?),
                    args: vec![odc_request(c, r, state, &t, expires)?]
                        .try_into()
                        .map_err(err)?,
                }),
                auth: vec![SorobanAuthorizationEntry {
                    credentials: SorobanCredentials::SourceAccount,
                    root_invocation: SorobanAuthorizedInvocation {
                        function: SorobanAuthorizedFunction::ContractFn(InvokeContractArgs {
                            contract_address: address(&c.wallet)?,
                            function_name: ScSymbol(b"execute".to_vec().try_into().map_err(err)?),
                            args: vec![odc_request(c, r, state, &t, expires)?]
                                .try_into()
                                .map_err(err)?,
                        }),
                        sub_invocations: VecM::default(),
                    },
                }]
                .try_into()
                .map_err(err)?,
            })
        }
    };
    Ok(Transaction {
        source_account: muxed(&state.source.address)?,
        fee: c.max_fee_stroops,
        seq_num: SequenceNumber(
            state
                .source
                .sequence
                .checked_add(1)
                .ok_or("Sequence overflow")?,
        ),
        cond: Preconditions::Time(TimeBounds {
            min_time: TimePoint(
                if c.route == WalletRoute::Native && !retained_native_lower_bound {
                    0
                } else {
                    now
                },
            ),
            max_time: TimePoint(expires),
        }),
        memo: if c.route == WalletRoute::Native {
            Memo::Hash(Hash(r.operation.nonce))
        } else {
            Memo::None
        },
        operations: vec![Operation {
            source_account: None,
            body,
        }]
        .try_into()
        .map_err(err)?,
        ext: TransactionExt::V0,
    })
}
pub fn prepare(
    c: &WalletConfig,
    r: &ExecutionRequest,
    state: &TrustedState,
    now: u64,
    expires_at: u64,
) -> Result<PreparedOperation> {
    prepare_with_bounds(c, r, state, now, expires_at, false)
}
fn prepare_with_bounds(
    c: &WalletConfig,
    r: &ExecutionRequest,
    state: &TrustedState,
    now: u64,
    expires_at: u64,
    retained_native_lower_bound: bool,
) -> Result<PreparedOperation> {
    let tx = build(c, r, state, now, expires_at, retained_native_lower_bound)?;
    Ok(PreparedOperation {
        version: 1,
        config: c.clone(),
        request: r.clone(),
        state: state.clone(),
        transaction_xdr: xdr(&TransactionEnvelope::Tx(TransactionV1Envelope {
            tx: tx.clone(),
            signatures: VecM::default(),
        }))?,
        transaction_hash: transaction_hash(&c.network, &tx)?,
        request_hash: hash(serde_json::to_vec(r).map_err(err)?),
        simulation_data: None,
        created_at: now,
        expires_at,
    })
}
/// Rebuild every authorization field before signing. Caller-supplied prepared metadata is not trusted.
pub fn validate_prepared(p: &PreparedOperation, now: u64) -> Result<()> {
    if p.version != 1 || p.created_at > now || now >= p.expires_at {
        return Err("Prepared operation is stale".into());
    }
    let retained_native_lower_bound = if p.config.route == WalletRoute::Native {
        match native_lower_bound(p)? {
            0 => false,
            value if value == p.created_at => true,
            _ => return Err("Native lower time bound changed".into()),
        }
    } else {
        false
    };
    let mut expected = prepare_with_bounds(
        &p.config,
        &p.request,
        &p.state,
        p.created_at,
        p.expires_at,
        retained_native_lower_bound,
    )?;
    if let Some(data) = &p.simulation_data {
        expected = attach_simulation(&expected, data)?;
    }
    if &expected != p {
        return Err("Prepared operation changed".into());
    }
    Ok(())
}
fn native_lower_bound(p: &PreparedOperation) -> Result<u64> {
    let TransactionEnvelope::Tx(envelope) = read(&p.transaction_xdr)? else {
        return Err("Unsupported transaction envelope".into());
    };
    let Preconditions::Time(bounds) = envelope.tx.cond else {
        return Err("Native time bounds are required".into());
    };
    Ok(bounds.min_time.0)
}
fn current_authorization_bounds(p: &PreparedOperation) -> Result<()> {
    if p.config.route == WalletRoute::Native && native_lower_bound(p)? != 0 {
        return Err("Retained native time bounds are read-only".into());
    }
    Ok(())
}
pub fn sign(p: &PreparedOperation, seed: &[u8; 32], now: u64) -> Result<DetachedSignature> {
    validate_prepared(p, now)?;
    current_authorization_bounds(p)?;
    verify_policy(&p.config, &p.request, &p.state, now)?;
    if p.config.route == WalletRoute::Odc && p.simulation_data.is_none() {
        return Err("ODC resource simulation is required before signing".into());
    }
    let signer = account_for_seed(seed);
    let allowed = if p.config.route == WalletRoute::Native {
        signer == p.config.agent || signer == p.config.engine
    } else {
        signer == p.config.agent
    };
    if !allowed {
        return Err("Signer has no automated role".into());
    }
    Ok(DetachedSignature {
        signer,
        signature: SigningKey::from_bytes(seed)
            .sign(&p.transaction_hash)
            .to_bytes()
            .to_vec(),
    })
}
pub fn authorize(
    p: &PreparedOperation,
    signatures: &[DetachedSignature],
    now: u64,
) -> Result<SignedOperation> {
    validate_prepared(p, now)?;
    current_authorization_bounds(p)?;
    let required = if p.config.route == WalletRoute::Native {
        vec![&p.config.agent, &p.config.engine]
    } else {
        vec![&p.config.agent]
    };
    if signatures.len() != required.len() {
        return Err("Exact automated signature quorum required".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in signatures {
        if !required.contains(&&s.signer) || !seen.insert(s.signer.clone()) {
            return Err("Unexpected or duplicate signature".into());
        }
        let key = account_key(&s.signer)?;
        let sig: [u8; 64] = s
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid signature length")?;
        VerifyingKey::from_bytes(&key)
            .map_err(err)?
            .verify_strict(&p.transaction_hash, &EdSignature::from_bytes(&sig))
            .map_err(err)?;
        out.push(DecoratedSignature {
            hint: SignatureHint(key[28..].try_into().unwrap()),
            signature: stellar_xdr::Signature(sig.to_vec().try_into().map_err(err)?),
        });
    }
    let TransactionEnvelope::Tx(mut env) = read(&p.transaction_xdr)? else {
        return Err("Unsupported transaction envelope".into());
    };
    env.signatures = out.try_into().map_err(err)?;
    Ok(SignedOperation {
        prepared: p.clone(),
        transaction_xdr: xdr(&TransactionEnvelope::Tx(env))?,
        transaction_hash: p.transaction_hash,
    })
}
pub fn validate_signed(s: &SignedOperation, now: u64) -> Result<()> {
    validate_prepared(&s.prepared, now)?;
    if s.transaction_hash != s.prepared.transaction_hash {
        return Err("Signed transaction identity differs".into());
    }
    let TransactionEnvelope::Tx(env) = read(&s.transaction_xdr)? else {
        return Err("Unsupported signed envelope".into());
    };
    let TransactionEnvelope::Tx(unsigned) = read(&s.prepared.transaction_xdr)? else {
        return Err("Unsupported unsigned envelope".into());
    };
    if env.tx != unsigned.tx {
        return Err("Signed transaction bytes differ".into());
    }
    let expected = if s.prepared.config.route == WalletRoute::Native {
        vec![&s.prepared.config.agent, &s.prepared.config.engine]
    } else {
        vec![&s.prepared.config.agent]
    };
    if env.signatures.len() != expected.len() {
        return Err("Signed transaction quorum differs".into());
    }
    let mut used = std::collections::BTreeSet::new();
    for sig in env.signatures.iter() {
        let mut matched = None;
        for signer in &expected {
            let key = account_key(signer)?;
            if sig.hint.0 == key[28..]
                && VerifyingKey::from_bytes(&key)
                    .map_err(err)?
                    .verify_strict(
                        &s.transaction_hash,
                        &EdSignature::from_slice(sig.signature.as_slice()).map_err(err)?,
                    )
                    .is_ok()
            {
                matched = Some((*signer).clone());
                break;
            }
        }
        let signer = matched.ok_or("Invalid transaction signature")?;
        if !used.insert(signer) {
            return Err("Duplicate transaction signer".into());
        }
    }
    Ok(())
}

/// Attach only RPC resource data. The exact invocation and authorization remain unchanged.
pub fn attach_simulation(p: &PreparedOperation, data: &str) -> Result<PreparedOperation> {
    if p.config.route != WalletRoute::Odc {
        return Err("Resource data is only valid for ODC execution".into());
    }
    let resource: SorobanTransactionData = read(data)?;
    let required = u32::try_from(resource.resource_fee)
        .map_err(|_| "Invalid resource fee")?
        .checked_add(100)
        .ok_or("Resource fee overflow")?;
    if required > p.config.max_fee_stroops {
        return Err("Simulation resource fee exceeds approved fee bound".into());
    }
    let TransactionEnvelope::Tx(mut env) = read(&p.transaction_xdr)? else {
        return Err("Unsupported transaction envelope".into());
    };
    env.tx.ext = TransactionExt::V1(resource);
    env.tx.fee = required;
    let mut out = p.clone();
    out.transaction_hash = transaction_hash(&p.config.network, &env.tx)?;
    out.transaction_xdr = xdr(&TransactionEnvelope::Tx(env))?;
    out.simulation_data = Some(data.into());
    Ok(out)
}
