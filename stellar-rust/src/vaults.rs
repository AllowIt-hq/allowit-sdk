//! Admitted vault effects and authenticated chain observations.
use crate::*;
use allowit_policy_sdk::typed_workflow::{
    AssetId, OperationRef, WorkflowEffectBound, WorkflowFeeBound,
};
use serde_json::json;

pub const DEPOSIT_ARGUMENT_SCHEMA: &[u8] =
    allowit_policy_sdk::stellar_vault::DEPOSIT_ARGUMENT_SCHEMA;
pub const DEPOSIT_OPERATION: &str = allowit_policy_sdk::stellar_vault::DEPOSIT_OPERATION;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultDeposit {
    pub vault: String,
    pub asset: String,
    pub holder: String,
    pub assets: i128,
    pub min_shares: i128,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StellarOperation {
    Transfer(Transfer),
    VaultDeposit(VaultDeposit),
}
impl StellarOperation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Transfer(_) => "stellar::sep41_transfer",
            Self::VaultDeposit(_) => DEPOSIT_OPERATION,
        }
    }
    /// Exact underlying movement. This projection alone does not prove share output.
    pub fn movement(&self) -> Transfer {
        match self {
            Self::Transfer(t) => t.clone(),
            Self::VaultDeposit(d) => Transfer {
                token: d.asset.clone(),
                source: d.holder.clone(),
                recipient: d.vault.clone(),
                amount: d.assets,
                native: false,
            },
        }
    }
    pub(crate) fn argument_schema(&self) -> &'static [u8] {
        match self {
            Self::Transfer(_) => ARGUMENT_SCHEMA,
            Self::VaultDeposit(_) => DEPOSIT_ARGUMENT_SCHEMA,
        }
    }
    pub(crate) fn arguments(&self, config: &WalletConfig) -> Result<ScVal> {
        match self {
            Self::Transfer(t) => transfer_args(config, t),
            Self::VaultDeposit(d) => value_map(vec![
                ("assets", i128_value(d.assets)),
                ("from", ScVal::Address(address(&d.holder)?)),
                ("min_shares", i128_value(d.min_shares)),
                ("operator", ScVal::Address(address(&d.holder)?)),
                ("receiver", ScVal::Address(address(&d.holder)?)),
                ("vault", ScVal::Address(address(&d.vault)?)),
            ]),
        }
    }
}
fn workflow(value: &str) -> Result<WorkflowAccount> {
    let (kind, bytes) = match address(value)? {
        ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(k))) => {
            (AccountKind::Wallet, k.0)
        }
        ScAddress::Contract(k) => (AccountKind::Contract, k.0.0),
        _ => return Err("Unsupported Stellar address".into()),
    };
    Ok(WorkflowAccount {
        chain: Chain::Stellar,
        kind,
        address: bytes,
    })
}
fn bound_amount(value: i128) -> Result<Amount256> {
    let n = u128::try_from(value).map_err(err)?;
    let mut out = [0; 32];
    out[16..].copy_from_slice(&n.to_be_bytes());
    Ok(Amount256(out))
}
/// Build a standard deposit call with separate underlying debit and minted-share bounds.
/// The custody wallet supplies the atomic minimum-output check absent from SEP-56's ABI.
pub fn vault_deposit_request(
    c: &WalletConfig,
    operation_id: [u8; 32],
    vault: &str,
    assets: i128,
    min_shares: i128,
    payer: &str,
) -> Result<ExecutionRequest> {
    let holder = workflow(&c.wallet)?;
    let vault_id = workflow(vault)?;
    let network = network_id(&c.network)?;
    let asset = |token| WorkflowAsset::Token(AssetId { network, token });
    let request = ExecutionRequest {
        operation: OperationRef {
            policy_instance: c.policy_instance,
            nonce: operation_id,
        },
        input: ExecutionInput::Instructions(vec![NativeInstruction::StellarCall {
            contract: vault_id.clone(),
            schema_digest: hash(DEPOSIT_ARGUMENT_SCHEMA),
            function: "deposit".into(),
            arguments: vec![
                AbiValue::Signed128(assets),
                AbiValue::Account(holder.clone()),
                AbiValue::Account(holder.clone()),
                AbiValue::Account(holder.clone()),
            ],
        }]),
        effect_bounds: vec![
            WorkflowEffectBound {
                asset: asset(workflow(&c.asset)?),
                source: holder.clone(),
                beneficiary: vault_id.clone(),
                max_debit: bound_amount(assets)?,
                min_credit: bound_amount(assets)?,
                max_burn: 0u64.into(),
            },
            WorkflowEffectBound {
                asset: asset(vault_id.clone()),
                source: vault_id,
                beneficiary: holder,
                max_debit: 0u64.into(),
                min_credit: bound_amount(min_shares)?,
                max_burn: 0u64.into(),
            },
        ],
        fee_bounds: Some(vec![WorkflowFeeBound {
            payer: workflow(payer)?,
            asset: WorkflowAsset::Native {
                chain: Chain::Stellar,
                network,
            },
            max_fee: u64::from(c.max_fee_stroops).into(),
        }]),
        evidence: vec![],
    };
    operation_from_request(c, &request)?;
    Ok(request)
}
/// Decode the entire admitted operation. Unsupported redemption fails before signing.
pub fn operation_from_request(c: &WalletConfig, r: &ExecutionRequest) -> Result<StellarOperation> {
    decode_operation(c, r, false)
}
pub(crate) fn decode_operation(
    c: &WalletConfig,
    r: &ExecutionRequest,
    retained: bool,
) -> Result<StellarOperation> {
    let ExecutionInput::Instructions(instructions) = &r.input else {
        return Err("Stellar execution requires typed instructions".into());
    };
    if instructions
        .iter()
        .any(|i| matches!(i,NativeInstruction::StellarCall{function,..}if function=="redeem"))
    {
        return Err("Delegated SEP-56 redemption is not admitted; use an owner-authorized redemption and verified reconciliation".into());
    }
    let [
        NativeInstruction::StellarCall {
            contract,
            schema_digest,
            function,
            arguments,
        },
    ] = instructions.as_slice()
    else {
        return decode_transfer(c, r, retained).map(StellarOperation::Transfer);
    };
    if function != "deposit" {
        return decode_transfer(c, r, retained).map(StellarOperation::Transfer);
    }
    validate_config(c)?;
    if c.route != WalletRoute::PolicyWallet {
        return Err("SEP-56 deposit requires policy-wallet atomic share bounds".into());
    }
    if r.operation.policy_instance != c.policy_instance
        || r.operation.nonce == [0; 32]
        || !r.evidence.is_empty()
        || r.effect_bounds.len() != 2
        || *schema_digest != hash(DEPOSIT_ARGUMENT_SCHEMA)
    {
        return Err("Invalid SEP-56 operation identity, schema or complete effect bounds".into());
    }
    let [
        AbiValue::Signed128(assets),
        AbiValue::Account(receiver),
        AbiValue::Account(from),
        AbiValue::Account(operator),
    ] = arguments.as_slice()
    else {
        return Err("Invalid standard SEP-56 deposit arguments".into());
    };
    let vault = workflow_address(contract)?;
    if contract.kind != AccountKind::Contract
        || vault == c.wallet
        || vault == c.asset
        || *assets <= 0
        || [receiver, from, operator]
            .iter()
            .any(|v| workflow_address(v).ok().as_deref() != Some(c.wallet.as_str()))
    {
        return Err(
            "SEP-56 deposit must retain owner-controlled shares and exact positive assets".into(),
        );
    }
    let network = network_id(&c.network)?;
    let underlying = &r.effect_bounds[0];
    let shares = &r.effect_bounds[1];
    let token_matches = |asset: &WorkflowAsset, token: &str| matches!(asset,WorkflowAsset::Token(a)if a.network==network&&workflow_address(&a.token).ok().as_deref()==Some(token));
    if !token_matches(&underlying.asset, &c.asset)
        || workflow_address(&underlying.source)? != c.wallet
        || workflow_address(&underlying.beneficiary)? != vault
        || amount(&underlying.max_debit)? != *assets
        || amount(&underlying.min_credit)? != *assets
        || amount(&underlying.max_burn)? != 0
        || !token_matches(&shares.asset, &vault)
        || workflow_address(&shares.source)? != vault
        || workflow_address(&shares.beneficiary)? != c.wallet
        || amount(&shares.max_debit)? != 0
        || amount(&shares.max_burn)? != 0
        || amount(&shares.min_credit)? <= 0
    {
        return Err("SEP-56 debit or share credit differs from approved effects".into());
    }
    let fees = r
        .fee_bounds
        .as_ref()
        .ok_or("Resolved SEP-56 fee bounds required")?;
    if fees.len() != 1
        || workflow_address(&fees[0].payer)? != c.agent
        || !matches!(&fees[0].asset,WorkflowAsset::Native{chain:Chain::Stellar,network:n}if *n==network)
        || amount(&fees[0].max_fee)? < i128::from(c.max_fee_stroops)
    {
        return Err("SEP-56 native fee payer or bound differs".into());
    }
    Ok(StellarOperation::VaultDeposit(VaultDeposit {
        vault,
        asset: c.asset.clone(),
        holder: c.wallet.clone(),
        assets: *assets,
        min_shares: amount(&shares.min_credit)?,
    }))
}

/// Created only after verifying configured-RPC settlement metadata against the original intent.
#[derive(Clone, Debug)]
pub struct VerifiedVaultDeposit {
    vault: String,
    asset: String,
    holder: String,
    assets: i128,
    shares: i128,
    ledger: u32,
    operation_id: [u8; 32],
    transaction_hash: [u8; 32],
}
impl VerifiedVaultDeposit {
    pub fn vault(&self) -> &str {
        &self.vault
    }
    pub fn asset(&self) -> &str {
        &self.asset
    }
    pub fn holder(&self) -> &str {
        &self.holder
    }
    pub fn assets(&self) -> i128 {
        self.assets
    }
    pub fn shares(&self) -> i128 {
        self.shares
    }
    pub fn ledger(&self) -> u32 {
        self.ledger
    }
    pub fn operation_id(&self) -> [u8; 32] {
        self.operation_id
    }
    pub fn transaction_hash(&self) -> [u8; 32] {
        self.transaction_hash
    }
}
#[derive(Clone, Debug)]
pub struct VerifiedVaultRedemption {
    vault: String,
    asset: String,
    holder: String,
    assets: i128,
    shares: i128,
    ledger: u32,
    transaction_hash: [u8; 32],
}
impl VerifiedVaultRedemption {
    pub fn vault(&self) -> &str {
        &self.vault
    }
    pub fn asset(&self) -> &str {
        &self.asset
    }
    pub fn holder(&self) -> &str {
        &self.holder
    }
    pub fn assets(&self) -> i128 {
        self.assets
    }
    pub fn shares(&self) -> i128 {
        self.shares
    }
    pub fn ledger(&self) -> u32 {
        self.ledger
    }
    pub fn transaction_hash(&self) -> [u8; 32] {
        self.transaction_hash
    }
}
#[derive(Clone, Debug)]
pub struct VaultPositionObservation {
    vault: String,
    asset: String,
    holder: String,
    shares: i128,
    redeemable: i128,
    share_decimals: u32,
    ledger: u32,
}
impl VaultPositionObservation {
    pub fn vault(&self) -> &str {
        &self.vault
    }
    pub fn asset(&self) -> &str {
        &self.asset
    }
    pub fn holder(&self) -> &str {
        &self.holder
    }
    pub fn shares(&self) -> i128 {
        self.shares
    }
    pub fn redeemable_units(&self) -> i128 {
        self.redeemable
    }
    pub fn share_decimals(&self) -> u32 {
        self.share_decimals
    }
    pub fn ledger(&self) -> u32 {
        self.ledger
    }
}
fn network(rpc: &dyn rpc::Rpc, c: &WalletConfig) -> Result<()> {
    validate_config(c)?;
    if rpc.call("getNetwork", json!({}))?["passphrase"] != passphrase(&c.network)? {
        return Err("Vault RPC network differs from installation".into());
    }
    Ok(())
}
fn settled(
    rpc: &dyn rpc::Rpc,
    c: &WalletConfig,
    txhash: &[u8; 32],
) -> Result<(rpc::VerifiedReceipt, Transaction, Vec<ContractEvent>, ScVal)> {
    network(rpc, c)?;
    let v = rpc.call("getTransaction", json!({"hash":hex(txhash)}))?;
    if rpc::verify_settlement(&v, &c.network, txhash)? != OperationStatus::Confirmed {
        return Err("Confirmed vault settlement is required".into());
    }
    let ledger = v["ledger"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or("Vault settlement ledger is required")?;
    let receipt = rpc::VerifiedReceipt {
        transaction_hash: *txhash,
        status: OperationStatus::Confirmed,
        ledger,
        envelope_xdr: v["envelopeXdr"]
            .as_str()
            .ok_or("Missing vault envelope")?
            .into(),
        result_xdr: v["resultXdr"]
            .as_str()
            .ok_or("Missing vault result")?
            .into(),
    };
    let TransactionEnvelope::Tx(envelope) = read(&receipt.envelope_xdr)? else {
        return Err("Unsupported vault envelope".into());
    };
    if envelope.tx.operations.len() != 1 {
        return Err("One exact vault operation is required".into());
    }
    let metadata: TransactionMeta = read(
        v["resultMetaXdr"]
            .as_str()
            .ok_or("Successful vault transaction metadata is required")?,
    )?;
    let (events, returned) = match metadata {
        TransactionMeta::V3(m) => {
            if m.operations.len() != 1 {
                return Err("Vault metadata operation count differs".into());
            };
            let sm = m.soroban_meta.ok_or("Missing Soroban vault metadata")?;
            (sm.events.to_vec(), sm.return_value)
        }
        TransactionMeta::V4(m) => {
            if m.operations.len() != 1 {
                return Err("Vault metadata operation count differs".into());
            };
            let sm = m.soroban_meta.ok_or("Missing Soroban vault metadata")?;
            (
                m.operations[0].events.to_vec(),
                sm.return_value.ok_or("Missing vault return value")?,
            )
        }
        _ => return Err("Unsupported vault settlement metadata".into()),
    };
    let result: TransactionResult = read(&receipt.result_xdr)?;
    let TransactionResultResult::TxSuccess(results) = result.result else {
        return Err("Vault transaction did not succeed".into());
    };
    let [
        OperationResult::OpInner(OperationResultTr::InvokeHostFunction(
            InvokeHostFunctionResult::Success(expected),
        )),
    ] = results.as_slice()
    else {
        return Err("Vault result is not one successful contract invocation".into());
    };
    let actual = hash(
        InvokeHostFunctionSuccessPreImage {
            return_value: returned.clone(),
            events: events.clone().try_into().map_err(err)?,
        }
        .to_xdr(limits())
        .map_err(err)?,
    );
    if expected.0 != actual {
        return Err("Vault metadata does not match the committed invocation result".into());
    }
    Ok((receipt, envelope.tx, events, returned))
}
fn invoke(tx: &Transaction) -> Result<&InvokeContractArgs> {
    let OperationBody::InvokeHostFunction(op) = &tx.operations[0].body else {
        return Err("Expected one vault contract invocation".into());
    };
    let HostFunction::InvokeContract(call) = &op.host_function else {
        return Err("Expected vault contract call".into());
    };
    Ok(call)
}
fn event_body<'a>(event: &'a ContractEvent, contract: &str) -> Result<Option<&'a ContractEventV0>> {
    if event.type_ != ContractEventType::Contract {
        return Ok(None);
    }
    let ScAddress::Contract(id) = address(contract)? else {
        return Err("Expected token contract".into());
    };
    if event.contract_id.as_ref() != Some(&id) {
        return Ok(None);
    }
    let ContractEventBody::V0(body) = &event.body;
    Ok(Some(body))
}
fn event_amount(data: &ScVal) -> Result<i128> {
    let value = if matches!(data, ScVal::Map(_)) {
        rpc::map_field(data, "amount")?
    } else {
        data
    };
    let n = rpc::wide(value)?;
    if n <= 0 {
        return Err("Vault token effect must be positive".into());
    }
    Ok(n)
}
fn token_effect(
    events: &[ContractEvent],
    token: &str,
    kind: &str,
    from: &str,
    to: Option<&str>,
) -> Result<i128> {
    let mut total = 0i128;
    for event in events {
        let Some(body) = event_body(event, token)? else {
            continue;
        };
        if body.topics.first() != Some(&value_symbol(kind)?) {
            continue;
        }
        let count = if to.is_some() { 3 } else { 2 };
        if body.topics.len() < count || body.topics[1] != ScVal::Address(address(from)?) {
            continue;
        }
        if let Some(to) = to {
            if body.topics[2] != ScVal::Address(address(to)?) {
                continue;
            }
        }
        if let Ok(muxed) = rpc::map_field(&body.data, "to_muxed_id") {
            if *muxed != ScVal::Void {
                return Err("Multiplexed vault token effects are not admitted".into());
            }
        }
        total = total
            .checked_add(event_amount(&body.data)?)
            .ok_or("Vault effect total overflow")?;
    }
    Ok(total)
}
/// Prove minted shares from committed metadata, not a quote or mutable current receipt getter.
pub fn vault_deposit_receipt(
    rpc: &dyn rpc::Rpc,
    p: &PreparedOperation,
    receipt: &rpc::VerifiedReceipt,
) -> Result<VerifiedVaultDeposit> {
    validate_retained_prepared(p)?;
    let StellarOperation::VaultDeposit(d) = operation_from_request(&p.config, &p.request)? else {
        return Err("Expected retained SEP-56 deposit".into());
    };
    let (actual, tx, events, returned) = settled(rpc, &p.config, &receipt.transaction_hash)?;
    if actual != *receipt {
        return Err("Vault receipt differs from configured RPC settlement".into());
    }
    let TransactionEnvelope::Tx(original) = read(&p.transaction_xdr)? else {
        return Err("Expected original vault packet".into());
    };
    if original.tx != tx {
        return Err("Vault settlement differs from original prepared intent".into());
    }
    let expected = value_map(vec![
        ("operation_id", value_bytes(&p.request.operation.nonce)?),
        ("token", ScVal::Address(address(&d.asset)?)),
        ("from", ScVal::Address(address(&d.holder)?)),
        ("to", ScVal::Address(address(&d.vault)?)),
        ("amount", i128_value(d.assets)),
        ("nonce", ScVal::U64(p.state.nonce)),
        ("deployment_version", ScVal::U64(p.state.deployment_version)),
    ])?;
    if returned != expected {
        return Err("Vault settlement returned a different underlying receipt".into());
    }
    let mut minted = None;
    for event in &events {
        let Some(body) = event_body(event, &d.holder)? else {
            continue;
        };
        if body.topics.as_slice()
            == [
                value_symbol("sep56_deposit")?,
                value_bytes(&p.request.operation.nonce)?,
                ScVal::Address(address(&d.vault)?),
            ]
        {
            if minted.is_some() || rpc::wide(rpc::map_field(&body.data, "assets")?)? != d.assets {
                return Err("Duplicate or inconsistent vault deposit event".into());
            }
            minted = Some(rpc::wide(rpc::map_field(&body.data, "shares")?)?);
        }
    }
    let shares = minted
        .filter(|n| *n >= d.min_shares)
        .ok_or("Missing verified minimum vault share credit")?;
    if token_effect(&events, &d.asset, "transfer", &d.holder, Some(&d.vault))? != d.assets
        || token_effect(&events, &d.vault, "mint", &d.holder, None)? != shares
    {
        return Err("Vault deposit token effects differ from its committed receipt".into());
    }
    Ok(VerifiedVaultDeposit {
        vault: d.vault,
        asset: d.asset,
        holder: d.holder,
        assets: d.assets,
        shares,
        ledger: actual.ledger,
        operation_id: p.request.operation.nonce,
        transaction_hash: actual.transaction_hash,
    })
}
/// Reconcile a real owner-authorized redemption. This method never prepares or signs redemption.
pub fn vault_redemption_receipt(
    rpc: &dyn rpc::Rpc,
    c: &WalletConfig,
    txhash: &[u8; 32],
    vault: &str,
    min_ledger: u32,
) -> Result<VerifiedVaultRedemption> {
    let (receipt, tx, events, returned) = settled(rpc, c, txhash)?;
    if receipt.ledger < min_ledger {
        return Err("Redemption predates the retained vault position".into());
    }
    let call = invoke(&tx)?;
    let redeem_args = if c.route == WalletRoute::PolicyWallet {
        if address_text(&call.contract_address)? != c.wallet
            || call.function_name.0.as_slice() != b"invoke"
            || call.args.len() != 4
        {
            return Err("Expected exact owner wallet invocation".into());
        }
        if call.args[0] != ScVal::Address(address(vault)?)
            || call.args[1] != value_symbol("redeem")?
            || !matches!(&call.args[3], ScVal::Vec(Some(_)))
        {
            return Err("Owner packet does not invoke the selected vault redemption".into());
        }
        let ScVal::Vec(Some(args)) = &call.args[2] else {
            return Err("Invalid owner redemption arguments".into());
        };
        args.as_slice()
    } else if c.route == WalletRoute::Native {
        if address_text(&call.contract_address)? != vault
            || call.function_name.0.as_slice() != b"redeem"
            || tx.source_account != muxed(&c.wallet)?
        {
            return Err("Expected direct owner-controlled native redemption".into());
        };
        call.args.as_slice()
    } else {
        return Err("Historical wallet redemption is not admitted".into());
    };
    let holder_address = ScVal::Address(address(&c.wallet)?);
    if redeem_args.len() != 4 || redeem_args[1..].iter().any(|a| *a != holder_address) {
        return Err("Redemption must return assets to the original owner-controlled holder".into());
    }
    let shares = rpc::wide(&redeem_args[0])?;
    let assets = rpc::wide(&returned)?;
    if shares <= 0
        || assets <= 0
        || token_effect(&events, vault, "burn", &c.wallet, None)? != shares
        || token_effect(&events, &c.asset, "transfer", vault, Some(&c.wallet))? != assets
    {
        return Err("Redemption share burn or actual underlying credit differs".into());
    }
    // An additional underlying debit would invalidate the net credit represented above.
    for event in &events {
        if let Some(body) = event_body(event, &c.asset)? {
            if body.topics.first() == Some(&value_symbol("transfer")?)
                && body.topics.get(1) == Some(&ScVal::Address(address(&c.wallet)?))
            {
                return Err("Redemption also debits the holder's underlying asset".into());
            }
        }
    }
    Ok(VerifiedVaultRedemption {
        vault: vault.into(),
        asset: c.asset.clone(),
        holder: c.wallet.clone(),
        assets,
        shares,
        ledger: receipt.ledger,
        transaction_hash: *txhash,
    })
}
fn observed_query(
    rpc: &dyn rpc::Rpc,
    c: &WalletConfig,
    state: &TrustedState,
    contract: &str,
    function: &str,
    args: Vec<ScVal>,
    min_ledger: u32,
) -> Result<(ScVal, u32)> {
    let result = rpc::readonly_simulation(rpc, c, &state.source, contract, function, args)?;
    if result.get("restorePreamble").is_some_and(|v| !v.is_null()) {
        return Err("Vault observation requires restoration".into());
    }
    let ledger = result["latestLedger"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= min_ledger && *v > 0)
        .ok_or("Vault observation predates required settlement")?;
    Ok((rpc::readonly_result(&result)?, ledger))
}
/// Observe the held shares' current net redeemable amount through configured trusted RPC.
/// The observation is not a redemption or a release of temporary-loss accounting.
pub fn vault_position_observation(
    rpc: &dyn rpc::Rpc,
    c: &WalletConfig,
    state: &TrustedState,
    vault: &str,
    shares: i128,
    min_ledger: u32,
) -> Result<VaultPositionObservation> {
    network(rpc, c)?;
    if c.route != WalletRoute::PolicyWallet || shares <= 0 {
        return Err("Positive policy-wallet vault position required".into());
    }
    Contract::from_string(vault).map_err(err)?;
    let mut ledger = u32::MAX;
    let (asset, l) = observed_query(rpc, c, state, vault, "query_asset", vec![], min_ledger)?;
    ledger = ledger.min(l);
    if asset != ScVal::Address(address(&c.asset)?) {
        return Err("Vault underlying asset differs from installation".into());
    }
    let (balance, l) = observed_query(
        rpc,
        c,
        state,
        vault,
        "balance",
        vec![ScVal::Address(address(&c.wallet)?)],
        min_ledger,
    )?;
    ledger = ledger.min(l);
    if rpc::wide(&balance)? < shares {
        return Err("Observed holder share balance is below the tracked position".into());
    }
    let (decimals, l) = observed_query(rpc, c, state, vault, "decimals", vec![], min_ledger)?;
    ledger = ledger.min(l);
    let ScVal::U32(share_decimals) = decimals else {
        return Err("Invalid vault share decimal scale".into());
    };
    let (redeemable, l) = observed_query(
        rpc,
        c,
        state,
        vault,
        "preview_redeem",
        vec![i128_value(shares)],
        min_ledger,
    )?;
    ledger = ledger.min(l);
    let redeemable = rpc::wide(&redeemable)?;
    if redeemable < 0 {
        return Err("Vault redeemable amount is negative".into());
    }
    Ok(VaultPositionObservation {
        vault: vault.into(),
        asset: c.asset.clone(),
        holder: c.wallet.clone(),
        shares,
        redeemable,
        share_decimals,
        ledger,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ChainRpc {
        value: serde_json::Value,
    }
    impl rpc::Rpc for ChainRpc {
        fn call(&self, method: &str, _params: serde_json::Value) -> Result<serde_json::Value> {
            match method {
                "getNetwork" => Ok(json!({"passphrase":passphrase("stellar:testnet")?})),
                "getTransaction" => Ok(self.value.clone()),
                _ => Err(format!("Unexpected RPC {method}")),
            }
        }
    }
    fn setup() -> (WalletConfig, TrustedState) {
        let agent = account_for_seed(&[2; 32]);
        let server = account_for_seed(&[3; 32]);
        let source =
            "use allowit::v1::prelude::*; async fn _execute(ctx:&Context)->PolicyResult { Ok(()) }"
                .to_owned();
        let mut c = WalletConfig {
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: Contract([4; 32]).to_string(),
            owner: account_for_seed(&[1; 32]),
            agent: agent.clone(),
            server: server.clone(),
            route: WalletRoute::PolicyWallet,
            policy_engine: Some(PolicyEngine::SimplePolicyEngine),
            asset: String::new(),
            asset_symbol: "XLM".into(),
            asset_issuer: None,
            policy_source: source.clone(),
            source_hash: hash(source),
            module_digest: [5; 32],
            max_fee_stroops: 50_000,
        };
        c.asset = sac_address(&c.network, &Asset::Native).unwrap();
        let state = TrustedState {
            contract_version: Some(1),
            policy_state: Some(PolicyStateSnapshot {
                grant: NativeValues::default(),
                spent: NativeValues::default(),
            }),
            source: AccountState {
                address: agent.clone(),
                sequence: 40,
                master_weight: 1,
                low_threshold: 0,
                medium_threshold: 0,
                high_threshold: 0,
                signers: vec![],
            },
            nonce: 9,
            deployment_version: 4,
            spent_daily: 0,
            spent_total: 0,
            spent_day: 1,
            calls: 0,
            ledger: 100,
            module_digest: c.module_digest,
            source_hash: c.source_hash,
            owner: c.owner.clone(),
            agent,
            server,
            policy_instance: c.policy_instance,
        };
        (c, state)
    }
    fn event(contract: &str, topics: Vec<ScVal>, data: ScVal) -> ContractEvent {
        let ScAddress::Contract(id) = address(contract).unwrap() else {
            panic!()
        };
        ContractEvent {
            ext: ExtensionPoint::V0,
            contract_id: Some(id),
            type_: ContractEventType::Contract,
            body: ContractEventBody::V0(ContractEventV0 {
                topics: topics.try_into().unwrap(),
                data,
            }),
        }
    }
    fn committed(
        c: &WalletConfig,
        tx: Transaction,
        events: Vec<ContractEvent>,
        returned: ScVal,
    ) -> (ChainRpc, rpc::VerifiedReceipt) {
        let preimage = InvokeHostFunctionSuccessPreImage {
            return_value: returned.clone(),
            events: events.clone().try_into().unwrap(),
        };
        let result = TransactionResult {
            fee_charged: 100,
            result: TransactionResultResult::TxSuccess(
                vec![OperationResult::OpInner(
                    OperationResultTr::InvokeHostFunction(InvokeHostFunctionResult::Success(Hash(
                        hash(preimage.to_xdr(limits()).unwrap()),
                    ))),
                )]
                .try_into()
                .unwrap(),
            ),
            ext: TransactionResultExt::V0,
        };
        let meta = TransactionMeta::V4(TransactionMetaV4 {
            operations: vec![OperationMetaV2 {
                events: events.try_into().unwrap(),
                ..Default::default()
            }]
            .try_into()
            .unwrap(),
            soroban_meta: Some(SorobanTransactionMetaV2 {
                return_value: Some(returned),
                ..Default::default()
            }),
            ..Default::default()
        });
        let receipt = rpc::VerifiedReceipt {
            transaction_hash: transaction_hash(&c.network, &tx).unwrap(),
            status: OperationStatus::Confirmed,
            ledger: 500,
            envelope_xdr: xdr(&TransactionEnvelope::Tx(TransactionV1Envelope {
                tx,
                signatures: VecM::default(),
            }))
            .unwrap(),
            result_xdr: xdr(&result).unwrap(),
        };
        let value = json!({"status":"SUCCESS","ledger":500,"envelopeXdr":receipt.envelope_xdr,"resultXdr":receipt.result_xdr,"resultMetaXdr":xdr(&meta).unwrap()});
        (ChainRpc { value }, receipt)
    }
    fn deposit() -> (
        WalletConfig,
        PreparedOperation,
        ChainRpc,
        rpc::VerifiedReceipt,
    ) {
        let (c, state) = setup();
        let vault = Contract([8; 32]).to_string();
        let request = vault_deposit_request(&c, [9; 32], &vault, 1000, 90, &c.agent).unwrap();
        let prepared = prepare(&c, &request, &state, 86401, 86500).unwrap();
        let TransactionEnvelope::Tx(env) = read(&prepared.transaction_xdr).unwrap() else {
            panic!()
        };
        let events = vec![
            event(
                &c.asset,
                vec![
                    value_symbol("transfer").unwrap(),
                    ScVal::Address(address(&c.wallet).unwrap()),
                    ScVal::Address(address(&vault).unwrap()),
                ],
                i128_value(1000),
            ),
            event(
                &vault,
                vec![
                    value_symbol("mint").unwrap(),
                    ScVal::Address(address(&c.wallet).unwrap()),
                ],
                i128_value(95),
            ),
            event(
                &c.wallet,
                vec![
                    value_symbol("sep56_deposit").unwrap(),
                    value_bytes(&[9; 32]).unwrap(),
                    ScVal::Address(address(&vault).unwrap()),
                ],
                value_map(vec![
                    ("assets", i128_value(1000)),
                    ("shares", i128_value(95)),
                ])
                .unwrap(),
            ),
        ];
        let returned = value_map(vec![
            ("operation_id", value_bytes(&[9; 32]).unwrap()),
            ("token", ScVal::Address(address(&c.asset).unwrap())),
            ("from", ScVal::Address(address(&c.wallet).unwrap())),
            ("to", ScVal::Address(address(&vault).unwrap())),
            ("amount", i128_value(1000)),
            ("nonce", ScVal::U64(state.nonce)),
            ("deployment_version", ScVal::U64(state.deployment_version)),
        ])
        .unwrap();
        let (rpc, receipt) = committed(&c, env.tx, events, returned);
        (c, prepared, rpc, receipt)
    }
    #[test]
    fn exact_deposit_intent_keeps_share_bounds_in_generated_execute_packet() {
        let (c, p, _, _) = deposit();
        let StellarOperation::VaultDeposit(d) = operation_from_request(&c, &p.request).unwrap()
        else {
            panic!()
        };
        assert_eq!(d.min_shares, 90);
        assert!(transfer_from_request(&c, &p.request).is_err());
        let mut changed = p.request.clone();
        changed.effect_bounds[1].max_debit = 1u64.into();
        assert!(operation_from_request(&c, &changed).is_err());
        let mut native = c.clone();
        native.route = WalletRoute::Native;
        native.wallet = native.owner.clone();
        assert!(operation_from_request(&native, &p.request).is_err());
    }
    #[test]
    fn deposit_receipt_requires_committed_original_metadata_and_actual_share_mint() {
        let (_, p, rpc, receipt) = deposit();
        let verified = vault_deposit_receipt(&rpc, &p, &receipt).unwrap();
        assert_eq!(verified.shares(), 95);
        assert_eq!(verified.assets(), 1000);
        let mut tampered = rpc.value.clone();
        tampered["resultMetaXdr"] =
            json!(xdr(&TransactionMeta::V4(TransactionMetaV4::default())).unwrap());
        assert!(vault_deposit_receipt(&ChainRpc { value: tampered }, &p, &receipt).is_err());
        let mut stale = receipt.clone();
        stale.ledger = 499;
        assert!(vault_deposit_receipt(&rpc, &p, &stale).is_err());
    }
    #[test]
    fn owner_redemption_requires_exact_invocation_burn_and_underlying_credit() {
        let (c, p, _, _) = deposit();
        let vault = Contract([8; 32]).to_string();
        let TransactionEnvelope::Tx(mut env) = read(&p.transaction_xdr).unwrap() else {
            panic!()
        };
        let args = ScVal::Vec(Some(
            vec![
                i128_value(95),
                ScVal::Address(address(&c.wallet).unwrap()),
                ScVal::Address(address(&c.wallet).unwrap()),
                ScVal::Address(address(&c.wallet).unwrap()),
            ]
            .try_into()
            .unwrap(),
        ));
        env.tx.operations = vec![Operation {
            source_account: None,
            body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: HostFunction::InvokeContract(InvokeContractArgs {
                    contract_address: address(&c.wallet).unwrap(),
                    function_name: ScSymbol(b"invoke".to_vec().try_into().unwrap()),
                    args: vec![
                        ScVal::Address(address(&vault).unwrap()),
                        value_symbol("redeem").unwrap(),
                        args,
                        ScVal::Vec(Some(ScVec::default())),
                    ]
                    .try_into()
                    .unwrap(),
                }),
                auth: VecM::default(),
            }),
        }]
        .try_into()
        .unwrap();
        let events = vec![
            event(
                &vault,
                vec![
                    value_symbol("burn").unwrap(),
                    ScVal::Address(address(&c.wallet).unwrap()),
                ],
                value_map(vec![("amount", i128_value(95))]).unwrap(),
            ),
            event(
                &c.asset,
                vec![
                    value_symbol("transfer").unwrap(),
                    ScVal::Address(address(&vault).unwrap()),
                    ScVal::Address(address(&c.wallet).unwrap()),
                ],
                i128_value(990),
            ),
        ];
        let (rpc, receipt) = committed(&c, env.tx, events, i128_value(990));
        let proof =
            vault_redemption_receipt(&rpc, &c, &receipt.transaction_hash, &vault, 499).unwrap();
        assert_eq!(proof.assets(), 990);
        assert_eq!(proof.shares(), 95);
        assert!(
            vault_redemption_receipt(&rpc, &c, &receipt.transaction_hash, &vault, 501).is_err()
        );
        assert!(
            vault_redemption_receipt(
                &rpc,
                &c,
                &receipt.transaction_hash,
                &Contract([10; 32]).to_string(),
                499
            )
            .is_err()
        );
    }
}
