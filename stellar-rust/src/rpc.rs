//! Bounded JSON-RPC transport. Submission never retries an uncertain transaction.
use crate::*;
use serde_json::{Value, json};
use std::{
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

fn transaction_hash_for(network: &str, tx: &Transaction) -> Result<[u8; 32]> {
    super::transaction_hash(network, tx)
}
pub trait Rpc: Send + Sync {
    fn call(&self, method: &str, params: Value) -> Result<Value>;
}
pub struct HttpRpc {
    url: String,
    client: reqwest::blocking::Client,
    id: AtomicU64,
}
impl HttpRpc {
    pub fn new(url: &str) -> Result<Self> {
        let parsed = reqwest::Url::parse(url).map_err(err)?;
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || !(parsed.scheme() == "https"
                || (parsed.scheme() == "http"
                    && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
        {
            return Err("RPC requires HTTPS or local HTTP without credentials".into());
        }
        Ok(Self {
            url: url.into(),
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(err)?,
            id: AtomicU64::new(1),
        })
    }
}
impl Rpc for HttpRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id.fetch_add(1, Ordering::Relaxed);
        let response = self
            .client
            .post(&self.url)
            .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()
            .map_err(|_| "Stellar RPC transport result is unknown")?;
        if !response.status().is_success() {
            return Err("Stellar RPC rejected the HTTP request".into());
        }
        let mut bytes = Vec::new();
        response
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > MAX_BYTES {
            return Err("RPC response exceeds limit".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(err)?;
        if value["id"] != id || value["jsonrpc"] != "2.0" || value.get("error").is_some() {
            return Err("Invalid or failed Stellar RPC response".into());
        }
        value
            .get("result")
            .cloned()
            .ok_or("Missing RPC result".into())
    }
}
pub fn get_account(rpc: &dyn Rpc, address: &str) -> Result<AccountState> {
    let key = LedgerKey::Account(LedgerKeyAccount {
        account_id: AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(account_key(
            address,
        )?))),
    });
    let value = rpc.call("getLedgerEntries", json!({"keys":[xdr(&key)?]}))?;
    let entries = value["entries"]
        .as_array()
        .ok_or("Missing account entries")?;
    if entries.len() != 1 {
        return Err("Native account not found".into());
    }
    let raw = entries[0]["xdr"].as_str().ok_or("Missing account XDR")?;
    let LedgerEntryData::Account(account) = read(raw)? else {
        return Err("Wrong ledger entry type".into());
    };
    let PublicKey::PublicKeyTypeEd25519(key) = account.account_id.0;
    let actual = ed25519::PublicKey(key.0).to_string();
    if actual != address {
        return Err("Ledger account identity differs".into());
    }
    let mut signers = Vec::new();
    for s in account.signers.iter() {
        let SignerKey::Ed25519(key) = &s.key else {
            return Err("Hash, preauthorized and signed-payload signers are not admitted".into());
        };
        signers.push(AccountSigner {
            key: ed25519::PublicKey(key.0).to_string(),
            weight: s.weight,
        });
    }
    Ok(AccountState {
        address: actual,
        sequence: account.seq_num.0,
        master_weight: u32::from(account.thresholds.0[0]),
        low_threshold: u32::from(account.thresholds.0[1]),
        medium_threshold: u32::from(account.thresholds.0[2]),
        high_threshold: u32::from(account.thresholds.0[3]),
        signers,
    })
}
pub fn simulate(rpc: &dyn Rpc, p: &PreparedOperation) -> Result<PreparedOperation> {
    let v = rpc.call(
        "simulateTransaction",
        json!({"transaction":p.transaction_xdr}),
    )?;
    if v.get("error").is_some() {
        return Err("ODC simulation rejected the operation".into());
    }
    attach_simulation(
        p,
        v["transactionData"]
            .as_str()
            .ok_or("Missing Soroban resource simulation")?,
    )
}
pub trait OperationStore {
    fn record(
        &mut self,
        operation_id: [u8; 32],
        signed: &SignedOperation,
        status: OperationStatus,
    ) -> Result<()>;
    fn load(&self, operation_id: [u8; 32]) -> Result<Option<(SignedOperation, OperationStatus)>>;
}
/// Persist exact signed bytes before broadcast. An uncertain response retains them for lookup.
pub fn submit(
    rpc: &dyn Rpc,
    store: &mut dyn OperationStore,
    signed: &SignedOperation,
    now: u64,
) -> Result<OperationStatus> {
    validate_signed(signed, now)?;
    let id = signed.prepared.request.operation.nonce;
    if let Some((previous, status)) = store.load(id)? {
        if previous != *signed {
            return Err("Operation ID already binds different signed bytes".into());
        }
        if matches!(status, OperationStatus::Confirmed | OperationStatus::Failed) {
            return Ok(status);
        }
        if status != OperationStatus::Prepared {
            let observed = status_by_hash(
                rpc,
                &signed.prepared.config.network,
                &signed.transaction_hash,
            )?;
            store.record(id, signed, observed.clone())?;
            return Ok(observed);
        }
    }
    store.record(id, signed, OperationStatus::SubmissionUnknown)?;
    match rpc.call(
        "sendTransaction",
        json!({"transaction":signed.transaction_xdr}),
    ) {
        Ok(v) => {
            if v["hash"].as_str() != Some(hex(&signed.transaction_hash).as_str()) {
                return Ok(OperationStatus::SubmissionUnknown);
            }
            let status = match v["status"].as_str() {
                Some("PENDING" | "DUPLICATE") => OperationStatus::Submitted,
                Some("ERROR") => status_by_hash(
                    rpc,
                    &signed.prepared.config.network,
                    &signed.transaction_hash,
                )
                .unwrap_or(OperationStatus::SubmissionUnknown),
                _ => OperationStatus::SubmissionUnknown,
            };
            store.record(id, signed, status.clone())?;
            Ok(status)
        }
        Err(_) => Ok(OperationStatus::SubmissionUnknown),
    }
}
pub fn status_by_hash(
    rpc: &dyn Rpc,
    network: &str,
    transaction_hash: &[u8; 32],
) -> Result<OperationStatus> {
    let v = rpc.call("getTransaction", json!({"hash":hex(transaction_hash)}))?;
    if matches!(v["status"].as_str(), Some("SUCCESS" | "FAILED")) {
        let envelope: TransactionEnvelope = read(
            v["envelopeXdr"]
                .as_str()
                .ok_or("Settlement envelope is required")?,
        )?;
        let TransactionEnvelope::Tx(envelope) = envelope else {
            return Err("Unexpected settlement envelope".into());
        };
        if &transaction_hash_for(network, &envelope.tx)? != transaction_hash {
            return Err("Settlement transaction identity differs".into());
        }
        let result: TransactionResult = read(
            v["resultXdr"]
                .as_str()
                .ok_or("Settlement result is required")?,
        )?;
        if v["status"] == "SUCCESS"
            && !matches!(result.result, TransactionResultResult::TxSuccess(_))
        {
            return Err("Settlement success and result differ".into());
        }
        if v["status"] == "FAILED" && matches!(result.result, TransactionResultResult::TxSuccess(_))
        {
            return Err("Settlement failure and result differ".into());
        }
    }
    Ok(match v["status"].as_str() {
        Some("SUCCESS") => OperationStatus::Confirmed,
        Some("FAILED") => OperationStatus::Failed,
        Some("NOT_FOUND") => OperationStatus::SubmissionUnknown,
        _ => return Err("Unknown Stellar settlement state".into()),
    })
}
pub fn recover(
    rpc: &dyn Rpc,
    store: &mut dyn OperationStore,
    operation_id: [u8; 32],
) -> Result<OperationStatus> {
    let (signed, previous) = store.load(operation_id)?.ok_or("No retained operation")?;
    if matches!(
        previous,
        OperationStatus::Confirmed | OperationStatus::Failed
    ) {
        return Ok(previous);
    }
    let status = status_by_hash(
        rpc,
        &signed.prepared.config.network,
        &signed.transaction_hash,
    )?;
    store.record(operation_id, &signed, status.clone())?;
    Ok(status)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeAccounting {
    pub nonce: u64,
    pub revision: u64,
    pub spent_daily: i128,
    pub spent_total: i128,
    pub spent_day: u64,
    pub calls: u64,
}
fn map_field<'a>(value: &'a ScVal, name: &str) -> Result<&'a ScVal> {
    let ScVal::Map(Some(map)) = value else {
        return Err("Expected contract record".into());
    };
    map.iter()
        .find(|e| matches!(&e.key,ScVal::Symbol(key) if key.0.as_slice()==name.as_bytes()))
        .map(|e| &e.val)
        .ok_or_else(|| format!("Missing contract field {name}"))
}
fn number(value: &ScVal) -> Result<u64> {
    match value {
        ScVal::U32(v) => Ok((*v).into()),
        ScVal::U64(v) => Ok(*v),
        _ => Err("Expected contract unsigned integer".into()),
    }
}
fn wide(value: &ScVal) -> Result<i128> {
    match value {
        ScVal::I128(v) => Ok((i128::from(v.hi) << 64) | i128::from(v.lo)),
        _ => Err("Expected native i128 amount".into()),
    }
}
fn digest_field(value: &ScVal) -> Result<[u8; 32]> {
    match value {
        ScVal::Bytes(v) => {
            v.0.as_slice()
                .try_into()
                .map_err(|_| "Expected bytes32".into())
        }
        _ => Err("Expected bytes32".into()),
    }
}
fn addr(value: &ScVal) -> Result<String> {
    match value {
        ScVal::Address(a) => address_text(a),
        _ => Err("Expected native contract address".into()),
    }
}
fn readonly_call(
    rpc: &dyn Rpc,
    c: &WalletConfig,
    source: &AccountState,
    function: &str,
) -> Result<ScVal> {
    let call = InvokeContractArgs {
        contract_address: address(&c.wallet)?,
        function_name: ScSymbol(function.as_bytes().to_vec().try_into().map_err(err)?),
        args: VecM::default(),
    };
    let tx = Transaction {
        source_account: muxed(&source.address)?,
        fee: 100,
        seq_num: SequenceNumber(source.sequence.checked_add(1).ok_or("Sequence overflow")?),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: vec![Operation {
            source_account: None,
            body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: HostFunction::InvokeContract(call),
                auth: VecM::default(),
            }),
        }]
        .try_into()
        .map_err(err)?,
        ext: TransactionExt::V0,
    };
    let value=rpc.call("simulateTransaction",json!({"transaction":xdr(&TransactionEnvelope::Tx(TransactionV1Envelope{tx,signatures:VecM::default()}))?}))?;
    if value.get("error").is_some() {
        return Err("Contract binding simulation failed".into());
    }
    let results = value["results"]
        .as_array()
        .ok_or("Missing binding result")?;
    if results.len() != 1 {
        return Err("Unexpected binding result count".into());
    }
    read(results[0]["xdr"].as_str().ok_or("Missing binding XDR")?)
}
/// Fetch native authority and ODC state from the configured chain. Native usage must
/// come from the authoritative durable coordinator, never an implicit zero counter.
pub fn trusted_state(
    rpc: &dyn Rpc,
    c: &WalletConfig,
    accounting: Option<NativeAccounting>,
) -> Result<TrustedState> {
    validate_config(c)?;
    let network = rpc.call("getNetwork", json!({}))?;
    if network["passphrase"].as_str() != Some(passphrase(&c.network)?) {
        return Err("Stellar RPC network differs from installation".into());
    }
    let latest = rpc.call("getLatestLedger", json!({}))?;
    let ledger = u32::try_from(
        latest["sequence"]
            .as_u64()
            .ok_or("Missing ledger sequence")?,
    )
    .map_err(err)?;
    let source = get_account(
        rpc,
        if c.route == WalletRoute::Native {
            &c.wallet
        } else {
            &c.agent
        },
    )?;
    if c.route == WalletRoute::Native {
        source.validate_wallet(c)?;
        let a = accounting.ok_or("Native wallet requires authoritative policy accounting")?;
        return Ok(TrustedState {
            source,
            nonce: a.nonce,
            revision: a.revision,
            spent_daily: a.spent_daily,
            spent_total: a.spent_total,
            spent_day: a.spent_day,
            calls: a.calls,
            ledger,
            module_digest: c.module_digest,
            source_hash: c.source_hash,
            owner: c.owner.clone(),
            agent: c.agent.clone(),
            engine: c.engine.clone(),
            policy_instance: c.policy_instance,
        });
    }
    let owner = get_account(rpc, &c.owner)?;
    if owner.master_weight == 0
        || !owner.signers.is_empty()
        || owner.low_threshold > owner.master_weight
        || owner.medium_threshold > owner.master_weight
        || owner.high_threshold > owner.master_weight
    {
        return Err("ODC owner account is not owner-only".into());
    }
    let key = LedgerKey::ContractData(LedgerKeyContractData {
        contract: address(&c.wallet)?,
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    });
    let entries = rpc.call("getLedgerEntries", json!({"keys":[xdr(&key)?]}))?;
    let list = entries["entries"]
        .as_array()
        .ok_or("Missing ODC ledger entry")?;
    if list.len() != 1 {
        return Err("ODC instance not found".into());
    }
    let entry: LedgerEntryData = read(list[0]["xdr"].as_str().ok_or("Missing instance XDR")?)?;
    let LedgerEntryData::ContractData(data) = entry else {
        return Err("Unexpected ODC ledger entry".into());
    };
    if data.contract != address(&c.wallet)?
        || data.key != ScVal::LedgerKeyContractInstance
        || data.durability != ContractDataDurability::Persistent
    {
        return Err("ODC instance ledger key differs".into());
    }
    let ScVal::ContractInstance(instance) = data.val else {
        return Err("Missing ODC instance state".into());
    };
    let ContractExecutable::Wasm(module) = instance.executable else {
        return Err("ODC must use an owner-selected Wasm executable".into());
    };
    if module.0 != c.module_digest {
        return Err("ODC executable differs from installed code".into());
    }
    let storage = instance.storage.ok_or("Missing ODC storage")?;
    let state = storage
        .iter()
        .find(|e| e.key == ScVal::U32(0))
        .map(|e| &e.val)
        .ok_or("Missing ODC custody state")?;
    if number(map_field(state, "version")?)? != 1 {
        return Err("Unsupported ODC custody state version".into());
    }
    if map_field(state, "paused")? != &ScVal::Bool(false) {
        return Err("ODC automation is paused".into());
    }
    let binding = readonly_call(rpc, c, &source, "policy_binding")?;
    if digest_field(map_field(&binding, "policy_instance")?)? != c.policy_instance
        || digest_field(map_field(&binding, "module_digest")?)? != c.module_digest
        || digest_field(map_field(&binding, "source_digest")?)? != c.source_hash
        || digest_field(map_field(&binding, "argument_schema_digest")?)? != hash(ARGUMENT_SCHEMA)
        || map_field(&binding, "entrypoint")? != &value_symbol("_execute")?
    {
        return Err("Compiled policy binding differs from installation".into());
    }
    let grant = map_field(state, "grant")?;
    if addr(map_field(grant, "asset")?)? != c.asset {
        return Err("ODC asset differs from installation".into());
    }
    let out = TrustedState {
        source,
        nonce: number(map_field(state, "nonce")?)?,
        revision: number(map_field(state, "revision")?)?,
        spent_daily: wide(map_field(state, "spent_daily")?)?,
        spent_total: wide(map_field(state, "spent_total")?)?,
        spent_day: number(map_field(state, "spent_day")?)?,
        calls: number(map_field(state, "calls")?)?,
        ledger,
        module_digest: module.0,
        source_hash: c.source_hash,
        owner: addr(map_field(state, "owner")?)?,
        agent: addr(map_field(state, "agent")?)?,
        engine: addr(map_field(state, "engine")?)?,
        policy_instance: c.policy_instance,
    };
    if out.owner != c.owner || out.agent != c.agent || out.engine != c.engine {
        return Err("ODC roles differ from installation".into());
    }
    Ok(out)
}
