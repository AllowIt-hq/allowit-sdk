use allowit_policy_sdk::typed_workflow::*;
use allowit_stellar_wallet::*;
use serde_json::{Value, json};
use std::sync::Mutex;
use stellar_strkey::Contract;
use stellar_xdr::{
    Limits, OperationBody, OperationResult, OperationResultTr, PaymentResult, ReadXdr,
    SorobanTransactionData, TransactionEnvelope, TransactionResult, TransactionResultExt,
    TransactionResultResult, WriteXdr,
};
fn wf(value: &str) -> AccountId {
    let address = address(value).unwrap();
    match address {
        stellar_xdr::ScAddress::Account(stellar_xdr::AccountId(
            stellar_xdr::PublicKey::PublicKeyTypeEd25519(key),
        )) => AccountId {
            chain: Chain::Stellar,
            kind: AccountKind::Wallet,
            address: key.0,
        },
        stellar_xdr::ScAddress::Contract(id) => AccountId {
            chain: Chain::Stellar,
            kind: AccountKind::Contract,
            address: id.0.0,
        },
        _ => panic!(),
    }
}
fn fixture(route: WalletRoute, usdc: bool) -> (WalletConfig, ExecutionRequest, TrustedState) {
    let owner = account_for_seed(&[1; 32]);
    let agent = account_for_seed(&[2; 32]);
    let engine = if route == WalletRoute::Native {
        account_for_seed(&[3; 32])
    } else {
        Contract([3; 32]).to_string()
    };
    let wallet = if route == WalletRoute::Native {
        owner.clone()
    } else {
        Contract([4; 32]).to_string()
    };
    let source =
        "use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult { Ok(()) }"
            .to_string();
    let mut c = WalletConfig {
        network: "stellar:testnet".into(),
        policy_instance: [7; 32],
        wallet,
        owner: owner.clone(),
        agent: agent.clone(),
        engine: engine.clone(),
        route: route.clone(),
        asset: String::new(),
        asset_symbol: if usdc { "USDC" } else { "XLM" }.into(),
        asset_issuer: usdc.then(|| account_for_seed(&[9; 32])),
        policy_source: source.clone(),
        source_hash: hash(source),
        module_digest: [5; 32],
        max_fee_stroops: 50_000,
    };
    c.asset = sac_address(&c.network, &asset_xdr(&c).unwrap()).unwrap();
    let recipient = account_for_seed(&[6; 32]);
    let token = WorkflowAsset::Token(AssetId {
        network: network_id(&c.network).unwrap(),
        token: wf(&c.asset),
    });
    let native = WorkflowAsset::Native {
        chain: Chain::Stellar,
        network: network_id(&c.network).unwrap(),
    };
    let units = 20_000_000u64;
    let instruction = if usdc {
        NativeInstruction::StellarCall {
            contract: wf(&c.asset),
            schema_digest: hash(ARGUMENT_SCHEMA),
            function: "transfer".into(),
            arguments: vec![
                AbiValue::Account(wf(&c.wallet)),
                AbiValue::Account(wf(&recipient)),
                AbiValue::Signed128(units.into()),
            ],
        }
    } else {
        NativeInstruction::StellarNativeTransfer {
            source: wf(&c.wallet),
            destination: wf(&recipient),
            stroops: units,
        }
    };
    let payer = if route == WalletRoute::Native {
        &c.wallet
    } else {
        &c.agent
    };
    let request = ExecutionRequest {
        operation: OperationRef {
            policy_instance: c.policy_instance,
            nonce: [8; 32],
        },
        input: ExecutionInput::Instructions(vec![instruction]),
        effect_bounds: vec![WorkflowEffectBound {
            asset: if usdc { token } else { native.clone() },
            source: wf(&c.wallet),
            beneficiary: wf(&recipient),
            max_debit: units.into(),
            min_credit: units.into(),
            max_burn: 0u64.into(),
        }],
        fee_bounds: Some(vec![WorkflowFeeBound {
            payer: wf(payer),
            asset: native,
            max_fee: 50_000u64.into(),
        }]),
        evidence: vec![],
    };
    let account = if route == WalletRoute::Native {
        AccountState {
            address: c.wallet.clone(),
            sequence: 40,
            master_weight: 3,
            low_threshold: 2,
            medium_threshold: 2,
            high_threshold: 3,
            signers: vec![
                AccountSigner {
                    key: agent,
                    weight: 1,
                },
                AccountSigner {
                    key: engine,
                    weight: 1,
                },
            ],
        }
    } else {
        AccountState {
            address: c.agent.clone(),
            sequence: 40,
            master_weight: 1,
            low_threshold: 0,
            medium_threshold: 0,
            high_threshold: 0,
            signers: vec![],
        }
    };
    let state = TrustedState {
        contract_version: (route == WalletRoute::Odc).then_some(1),
        source: account,
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
        agent: c.agent.clone(),
        engine: c.engine.clone(),
        policy_instance: c.policy_instance,
    };
    (c, request, state)
}
fn sign_native(p: &PreparedOperation) -> SignedOperation {
    let a = sign(p, &[2; 32], 86401).unwrap();
    let e = sign(p, &[3; 32], 86401).unwrap();
    authorize(p, &[a, e], 86401).unwrap()
}
#[test]
fn native_classic_payment_has_real_two_party_signatures() {
    for usdc in [false, true] {
        let (c, r, s) = fixture(WalletRoute::Native, usdc);
        let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
        assert!(authorize(&p, &[sign(&p, &[2; 32], 86401).unwrap()], 86401).is_err());
        let signed = sign_native(&p);
        validate_signed(&signed, 86401).unwrap();
        let TransactionEnvelope::Tx(tx) =
            TransactionEnvelope::from_xdr_base64(&signed.transaction_xdr, Limits::none()).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            tx.tx.memo,
            stellar_xdr::Memo::Hash(stellar_xdr::Hash(r.operation.nonce))
        );
        assert_eq!(tx.signatures.len(), 2);
        assert_eq!(tx.tx.seq_num.0, 41);
        let OperationBody::Payment(op) = &tx.tx.operations[0].body else {
            panic!()
        };
        assert_eq!(op.amount, 20_000_000);
        assert_eq!(op.asset, asset_xdr(&c).unwrap());
    }
}
#[test]
fn live_signers_and_thresholds_replace_the_fixed_template() {
    let (c, r, mut s) = fixture(WalletRoute::Native, false);
    s.source.signers.push(AccountSigner {
        key: account_for_seed(&[12; 32]),
        weight: 1,
    });
    assert!(prepare(&c, &r, &s, 86401, 86700).is_ok());
    s.source.high_threshold = 1;
    assert!(prepare(&c, &r, &s, 86401, 86700).is_ok());
    s.source.medium_threshold = 3;
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
    s.source.medium_threshold = 2;
    s.source.low_threshold = 3;
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
}
#[test]
fn separate_owner_profile_is_explicit_and_exact() {
    let (mut c, mut r, mut s) = fixture(WalletRoute::Native, false);
    c.owner = account_for_seed(&[10; 32]);
    s.owner = c.owner.clone();
    s.source.master_weight = 0;
    s.source.signers.push(AccountSigner {
        key: c.owner.clone(),
        weight: 3,
    });
    assert!(prepare(&c, &r, &s, 86401, 86700).is_ok());
    r.operation.policy_instance = [0; 32];
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
}
#[test]
fn stale_or_changed_payload_cannot_be_signed() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let mut p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    p.request.effect_bounds[0].min_credit = 1u64.into();
    assert!(sign(&p, &[2; 32], 86401).is_err());
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    assert!(sign(&p, &[2; 32], 86700).is_err());
    assert!(sign(&p, &[1; 32], 86401).is_err());
    let mut signed = sign_native(&p);
    signed.transaction_hash[0] ^= 1;
    assert!(validate_signed(&signed, 86401).is_err());
}
#[test]
fn same_authored_policy_gates_native_and_odc_without_rounding() {
    for route in [WalletRoute::Native, WalletRoute::Odc] {
        let (mut c, mut r, mut s) = fixture(route, true);
        c.policy_source="use allowit::v1::prelude::*; async fn _execute(ctx: &Context) -> PolicyResult { allowit::cap_per_transaction(ctx, \"2\", \"USDC\")?; Ok(()) }".into();
        c.source_hash = hash(&c.policy_source);
        s.source_hash = c.source_hash;
        assert!(prepare(&c, &r, &s, 86401, 86700).is_ok());
        let ExecutionInput::Instructions(v) = &mut r.input else {
            panic!()
        };
        let NativeInstruction::StellarCall { arguments, .. } = &mut v[0] else {
            panic!()
        };
        arguments[2] = AbiValue::Signed128(20_000_001);
        r.effect_bounds[0].max_debit = 20_000_001u64.into();
        r.effect_bounds[0].min_credit = 20_000_001u64.into();
        assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
    }
}
#[test]
fn odc_uses_execute_and_exact_module_schema_payload() {
    let (c, r, s) = fixture(WalletRoute::Odc, false);
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    assert!(sign(&p, &[2; 32], 86401).is_err());
    let data = SorobanTransactionData::default()
        .to_xdr_base64(Limits::none())
        .unwrap();
    let p = attach_simulation(&p, &data).unwrap();
    let sig = sign(&p, &[2; 32], 86401).unwrap();
    let signed = authorize(&p, &[sig], 86401).unwrap();
    validate_signed(&signed, 86401).unwrap();
    let TransactionEnvelope::Tx(tx) =
        TransactionEnvelope::from_xdr_base64(&signed.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    let OperationBody::InvokeHostFunction(op) = &tx.tx.operations[0].body else {
        panic!()
    };
    let stellar_xdr::HostFunction::InvokeContract(call) = &op.host_function else {
        panic!()
    };
    assert_eq!(tx.tx.memo, stellar_xdr::Memo::None);
    assert_eq!(call.function_name.0.as_slice(), b"execute");
    let stellar_xdr::ScVal::Map(Some(fields)) = &call.args[0] else {
        panic!()
    };
    let operation = fields.iter().find(|field| matches!(&field.key, stellar_xdr::ScVal::Symbol(key) if key.0.as_slice() == b"operation_id")).unwrap();
    let stellar_xdr::ScVal::Bytes(value) = &operation.val else {
        panic!()
    };
    assert_eq!(value.0.as_slice(), &r.operation.nonce);
    assert_eq!(call.args.len(), 1);
    assert_eq!(op.auth.len(), 1);
    let mut changed = s.clone();
    changed.module_digest[0] ^= 1;
    assert!(prepare(&c, &r, &changed, 86401, 86700).is_err());
}
#[derive(Default)]
struct Store {
    value: Option<(SignedOperation, OperationStatus)>,
}
impl rpc::OperationStore for Store {
    fn record(&mut self, _: [u8; 32], s: &SignedOperation, status: OperationStatus) -> Result<()> {
        if self
            .value
            .as_ref()
            .is_some_and(|(previous, _)| previous != s)
        {
            return Err("rebound operation".into());
        }
        self.value = Some((s.clone(), status));
        Ok(())
    }
    fn load(&self, _: [u8; 32]) -> Result<Option<(SignedOperation, OperationStatus)>> {
        Ok(self.value.clone())
    }
}
struct FakeRpc {
    calls: Mutex<Vec<String>>,
    signed: SignedOperation,
    fail_send: bool,
}
impl rpc::Rpc for FakeRpc {
    fn call(&self, method: &str, _: Value) -> Result<Value> {
        self.calls.lock().unwrap().push(method.into());
        match method {
            "sendTransaction" if self.fail_send => Err("connection lost".into()),
            "sendTransaction" => {
                Ok(json!({"hash":hex(&self.signed.transaction_hash),"status":"PENDING"}))
            }
            "getTransaction" => {
                let result = TransactionResult {
                    fee_charged: 100,
                    result: TransactionResultResult::TxSuccess(
                        vec![OperationResult::OpInner(OperationResultTr::Payment(
                            PaymentResult::Success,
                        ))]
                        .try_into()
                        .unwrap(),
                    ),
                    ext: TransactionResultExt::V0,
                };
                Ok(
                    json!({"status":"SUCCESS","ledger":777,"envelopeXdr":self.signed.transaction_xdr,"resultXdr":result.to_xdr_base64(Limits::none()).unwrap()}),
                )
            }
            _ => Err("unexpected rpc".into()),
        }
    }
}
#[test]
fn unknown_broadcast_recovers_original_transaction_without_resigning() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let signed = sign_native(&p);
    let rpc = FakeRpc {
        calls: Mutex::new(vec![]),
        signed: signed.clone(),
        fail_send: true,
    };
    let mut store = Store::default();
    assert_eq!(
        rpc::submit(&rpc, &mut store, &signed, 86401).unwrap(),
        OperationStatus::SubmissionUnknown
    );
    assert_eq!(
        rpc::recover(&rpc, &mut store, r.operation.nonce).unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(
        *rpc.calls.lock().unwrap(),
        vec!["sendTransaction", "getTransaction"]
    );
    assert_eq!(
        rpc::submit(&rpc, &mut store, &signed, 86401).unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(
        rpc.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.as_str() == "sendTransaction")
            .count(),
        1
    );
}
#[test]
fn requests_reject_other_rails_unbounded_fees_extra_effects_and_wrong_asset() {
    let (c, mut r, s) = fixture(WalletRoute::Native, false);
    r.fee_bounds = None;
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
    let (c, mut r, s) = fixture(WalletRoute::Native, false);
    r.effect_bounds.push(r.effect_bounds[0].clone());
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
    let (c, mut r, s) = fixture(WalletRoute::Native, false);
    let ExecutionInput::Instructions(v) = &mut r.input else {
        panic!()
    };
    v[0] = NativeInstruction::SolanaNativeTransfer {
        source: wf(&c.wallet),
        destination: wf(&account_for_seed(&[6; 32])),
        lamports: 20_000_000,
    };
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
}

struct ErrorAfterBroadcast {
    inner: FakeRpc,
    wrong_hash: bool,
    not_found: bool,
}
impl rpc::Rpc for ErrorAfterBroadcast {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        if method == "sendTransaction" {
            self.inner.calls.lock().unwrap().push(method.into());
            return Ok(
                json!({"status":"ERROR","hash":if self.wrong_hash{"wrong".into()}else{hex(&self.inner.signed.transaction_hash)},"errorResultXdr":"not proof of nonexecution"}),
            );
        }
        if method == "getTransaction" && self.not_found {
            self.inner.calls.lock().unwrap().push(method.into());
            return Ok(json!({"status":"NOT_FOUND"}));
        }
        self.inner.call(method, params)
    }
}
#[test]
fn submission_error_reconciles_original_hash_and_terminal_state_is_sticky() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let signed = sign_native(&prepare(&c, &r, &s, 86401, 86700).unwrap());
    let rpc = ErrorAfterBroadcast {
        inner: FakeRpc {
            calls: Mutex::new(vec![]),
            signed: signed.clone(),
            fail_send: false,
        },
        wrong_hash: false,
        not_found: false,
    };
    let mut store = Store::default();
    assert_eq!(
        rpc::submit(&rpc, &mut store, &signed, 86401).unwrap(),
        OperationStatus::Confirmed
    );
    let disappeared = ErrorAfterBroadcast {
        inner: FakeRpc {
            calls: Mutex::new(vec![]),
            signed,
            fail_send: false,
        },
        wrong_hash: false,
        not_found: true,
    };
    assert_eq!(
        rpc::recover(&disappeared, &mut store, r.operation.nonce).unwrap(),
        OperationStatus::Confirmed
    );
    assert!(disappeared.inner.calls.lock().unwrap().is_empty());
}
#[test]
fn hash_mismatch_and_unresolved_error_retain_uncertainty() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let signed = sign_native(&prepare(&c, &r, &s, 86401, 86700).unwrap());
    for wrong_hash in [true, false] {
        let rpc = ErrorAfterBroadcast {
            inner: FakeRpc {
                calls: Mutex::new(vec![]),
                signed: signed.clone(),
                fail_send: false,
            },
            wrong_hash,
            not_found: true,
        };
        let mut store = Store::default();
        assert_eq!(
            rpc::submit(&rpc, &mut store, &signed, 86401).unwrap(),
            OperationStatus::SubmissionUnknown
        );
        assert_eq!(
            rpc::recover(&rpc, &mut store, r.operation.nonce).unwrap(),
            OperationStatus::SubmissionUnknown
        );
    }
}
struct AccountRpc {
    config: WalletConfig,
    account: AccountState,
    wrong_network: bool,
}
impl rpc::Rpc for AccountRpc {
    fn call(&self, method: &str, _: Value) -> Result<Value> {
        match method {
            "getNetwork" => Ok(
                json!({"passphrase":if self.wrong_network{"wrong"}else{passphrase(&self.config.network)?}}),
            ),
            "getLatestLedger" => Ok(json!({"sequence":100})),
            "getLedgerEntries" => {
                let a = &self.account;
                let entry = stellar_xdr::AccountEntry {
                    account_id: stellar_xdr::AccountId(
                        stellar_xdr::PublicKey::PublicKeyTypeEd25519(stellar_xdr::Uint256(
                            account_key(&a.address)?,
                        )),
                    ),
                    seq_num: stellar_xdr::SequenceNumber(a.sequence),
                    thresholds: stellar_xdr::Thresholds([
                        a.master_weight as u8,
                        a.low_threshold as u8,
                        a.medium_threshold as u8,
                        a.high_threshold as u8,
                    ]),
                    signers: a
                        .signers
                        .iter()
                        .map(|s| {
                            Ok(stellar_xdr::Signer {
                                key: match stellar_strkey::Strkey::from_string(&s.key)
                                    .map_err(|e| e.to_string())?
                                {
                                    stellar_strkey::Strkey::PublicKeyEd25519(k) => {
                                        stellar_xdr::SignerKey::Ed25519(stellar_xdr::Uint256(k.0))
                                    }
                                    stellar_strkey::Strkey::PreAuthTx(k) => {
                                        stellar_xdr::SignerKey::PreAuthTx(stellar_xdr::Uint256(k.0))
                                    }
                                    stellar_strkey::Strkey::HashX(k) => {
                                        stellar_xdr::SignerKey::HashX(stellar_xdr::Uint256(k.0))
                                    }
                                    stellar_strkey::Strkey::SignedPayloadEd25519(k) => {
                                        stellar_xdr::SignerKey::Ed25519SignedPayload(
                                            stellar_xdr::SignerKeyEd25519SignedPayload {
                                                ed25519: stellar_xdr::Uint256(k.ed25519),
                                                payload: k.payload.try_into().unwrap(),
                                            },
                                        )
                                    }
                                    _ => return Err("not an account signer".into()),
                                },
                                weight: s.weight,
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                        .try_into()
                        .unwrap(),
                    ..Default::default()
                };
                Ok(
                    json!({"entries":[{"xdr":stellar_xdr::LedgerEntryData::Account(entry).to_xdr_base64(Limits::none()).unwrap()}]}),
                )
            }
            _ => Err("unexpected RPC".into()),
        }
    }
}
#[test]
fn native_state_uses_network_and_real_account_xdr_with_explicit_usage() {
    let (c, _, s) = fixture(WalletRoute::Native, false);
    let rpc = AccountRpc {
        config: c.clone(),
        account: s.source.clone(),
        wrong_network: false,
    };
    assert!(rpc::trusted_state(&rpc, &c, None).is_err());
    let accounting = rpc::NativeAccounting {
        nonce: 9,
        deployment_version: 4,
        spent_daily: 12,
        spent_total: 20,
        spent_day: 1,
        calls: 2,
    };
    let observed = rpc::trusted_state(&rpc, &c, Some(accounting.clone())).unwrap();
    assert_eq!(observed.source, s.source);
    assert_eq!(observed.spent_daily, 12);
    assert_eq!(observed.nonce, 9);
    let wrong = AccountRpc {
        config: c.clone(),
        account: s.source,
        wrong_network: true,
    };
    assert!(rpc::trusted_state(&wrong, &c, Some(accounting)).is_err());
}

#[test]
fn verified_receipt_retains_exact_network_transaction_and_ledger() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let signed = sign_native(&p);
    let rpc = FakeRpc {
        calls: Mutex::new(vec![]),
        signed: signed.clone(),
        fail_send: false,
    };
    let receipt = rpc::receipt_by_hash(&rpc, &c.network, &signed.transaction_hash)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.ledger, 777);
    assert_eq!(receipt.transaction_hash, signed.transaction_hash);
    assert_eq!(receipt.envelope_xdr, signed.transaction_xdr);
    assert_eq!(receipt.status, OperationStatus::Confirmed);
    rpc::validate_receipt(&receipt, &c.network, &signed.transaction_hash).unwrap();
    let mut changed = receipt.clone();
    changed.status = OperationStatus::Failed;
    assert!(rpc::validate_receipt(&changed, &c.network, &signed.transaction_hash).is_err());
    assert!(rpc::receipt_by_hash(&rpc, "stellar:mainnet", &signed.transaction_hash).is_err());
    assert!(rpc::receipt_by_hash(&rpc, &c.network, &[0; 32]).is_err());
}

#[test]
fn odc_resource_fees_require_explicit_config_and_request_budget() {
    let (mut config, mut request, state) = fixture(WalletRoute::Odc, false);
    config.max_fee_stroops = 1_000_000;
    request.fee_bounds.as_mut().unwrap()[0].max_fee = 1_000_000u64.into();
    let original = prepare(&config, &request, &state, 86401, 86700).unwrap();
    let mut resource = SorobanTransactionData {
        resource_fee: 1_331_294,
        ..Default::default()
    };
    let encode = |data: &SorobanTransactionData| data.to_xdr_base64(Limits::none()).unwrap();
    assert!(attach_simulation(&original, &encode(&resource)).is_err());
    config.max_fee_stroops = 2_000_000;
    assert!(prepare(&config, &request, &state, 86401, 86700).is_err());
    request.fee_bounds.as_mut().unwrap()[0].max_fee = 2_000_000u64.into();
    let approved = prepare(&config, &request, &state, 86401, 86700).unwrap();
    let prepared = attach_simulation(&approved, &encode(&resource)).unwrap();
    let TransactionEnvelope::Tx(envelope) =
        TransactionEnvelope::from_xdr_base64(&prepared.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    assert_eq!(envelope.tx.fee, 1_331_394);
    assert_eq!(envelope.tx.memo, stellar_xdr::Memo::None);
    for fee in [-1, i64::MAX, i64::from(u32::MAX), 1_999_901] {
        resource.resource_fee = fee;
        assert!(attach_simulation(&approved, &encode(&resource)).is_err());
    }
    resource.resource_fee = 1_999_900;
    assert!(attach_simulation(&approved, &encode(&resource)).is_ok());
    config.max_fee_stroops = 2_000_001;
    assert!(validate_config(&config).is_err());
    let (mut native, _, _) = fixture(WalletRoute::Native, false);
    native.max_fee_stroops = 1_000_000;
    validate_config(&native).unwrap();
    native.max_fee_stroops = 1_000_001;
    assert!(validate_config(&native).is_err());
}

fn native_packet_bounds(mut prepared: PreparedOperation, min: u64, max: u64) -> PreparedOperation {
    let TransactionEnvelope::Tx(mut envelope) =
        TransactionEnvelope::from_xdr_base64(&prepared.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    envelope.tx.cond = stellar_xdr::Preconditions::Time(stellar_xdr::TimeBounds {
        min_time: stellar_xdr::TimePoint(min),
        max_time: stellar_xdr::TimePoint(max),
    });
    prepared.transaction_hash = hash(
        stellar_xdr::TransactionSignaturePayload {
            network_id: stellar_xdr::Hash(network_id(&prepared.config.network).unwrap()),
            tagged_transaction: stellar_xdr::TransactionSignaturePayloadTaggedTransaction::Tx(
                envelope.tx.clone(),
            ),
        }
        .to_xdr(Limits::none())
        .unwrap(),
    );
    prepared.transaction_xdr = envelope_to_xdr(envelope);
    prepared
}
fn envelope_to_xdr(envelope: stellar_xdr::TransactionV1Envelope) -> String {
    TransactionEnvelope::Tx(envelope)
        .to_xdr_base64(Limits::none())
        .unwrap()
}
fn retained_native_signed(prepared: PreparedOperation) -> SignedOperation {
    use ed25519_dalek::{Signer, SigningKey};
    let TransactionEnvelope::Tx(mut envelope) =
        TransactionEnvelope::from_xdr_base64(&prepared.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    envelope.signatures = [2u8, 3]
        .into_iter()
        .map(|value| {
            let key = SigningKey::from_bytes(&[value; 32]);
            stellar_xdr::DecoratedSignature {
                hint: stellar_xdr::SignatureHint(
                    key.verifying_key().to_bytes()[28..].try_into().unwrap(),
                ),
                signature: stellar_xdr::Signature(
                    key.sign(&prepared.transaction_hash)
                        .to_bytes()
                        .to_vec()
                        .try_into()
                        .unwrap(),
                ),
            }
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    SignedOperation {
        transaction_xdr: envelope_to_xdr(envelope),
        transaction_hash: prepared.transaction_hash,
        prepared,
    }
}
#[test]
fn native_immediate_bounds_keep_exact_expiry_and_reject_other_lower_bounds() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let TransactionEnvelope::Tx(envelope) =
        TransactionEnvelope::from_xdr_base64(&p.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        envelope.tx.cond,
        stellar_xdr::Preconditions::Time(stellar_xdr::TimeBounds {
            min_time: stellar_xdr::TimePoint(0),
            max_time: stellar_xdr::TimePoint(86700)
        })
    );
    validate_signed(&sign_native(&p), 86401).unwrap();
    assert!(sign(&p, &[2; 32], 86700).is_err());
    assert!(sign(&p, &[2; 32], 86400).is_err());
    assert!(prepare(&c, &r, &s, 86401, 86702).is_err());
    for lower in [1, 86400, 86402, u64::MAX] {
        assert!(validate_prepared(&native_packet_bounds(p.clone(), lower, 86700), 86401).is_err());
    }
    for upper in [86699, 86701] {
        assert!(validate_prepared(&native_packet_bounds(p.clone(), 0, upper), 86401).is_err());
    }
}
#[test]
fn retained_native_status_preserves_signed_bytes_without_new_authorization() {
    let (c, r, s) = fixture(WalletRoute::Native, false);
    let current = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let retained = native_packet_bounds(current.clone(), 86401, 86700);
    validate_prepared(&retained, 86401).unwrap();
    assert!(
        sign(&retained, &[2; 32], 86401)
            .unwrap_err()
            .contains("read-only")
    );
    assert!(
        authorize(&retained, &[], 86401)
            .unwrap_err()
            .contains("read-only")
    );
    let signed = retained_native_signed(retained);
    validate_signed(&signed, 86401).unwrap();
    let rpc = FakeRpc {
        calls: Mutex::new(vec![]),
        signed: signed.clone(),
        fail_send: false,
    };
    let mut store = Store {
        value: Some((signed.clone(), OperationStatus::SubmissionUnknown)),
    };
    assert_eq!(
        rpc::recover(&rpc, &mut store, r.operation.nonce).unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(store.value.as_ref().unwrap().0, signed);
    let mut rewritten = signed.clone();
    let TransactionEnvelope::Tx(mut envelope) =
        TransactionEnvelope::from_xdr_base64(&signed.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    let TransactionEnvelope::Tx(new) =
        TransactionEnvelope::from_xdr_base64(&current.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    envelope.tx = new.tx;
    rewritten.prepared = current.clone();
    rewritten.transaction_hash = current.transaction_hash;
    rewritten.transaction_xdr = envelope_to_xdr(envelope);
    assert!(validate_signed(&rewritten, 86401).is_err());
    assert_eq!(*rpc.calls.lock().unwrap(), vec!["getTransaction"]);
}

#[test]
fn changed_customer_signer_is_resolved_from_current_native_account() {
    let (mut c, r, mut state) = fixture(WalletRoute::Native, false);
    let old = c.clone();
    c.agent = account_for_seed(&[4; 32]);
    state.agent = c.agent.clone();
    state.source.master_weight = 0;
    state.source.signers[0] = AccountSigner {
        key: c.agent.clone(),
        weight: 3,
    };
    state.source.signers[1].weight = 2;
    state.source.medium_threshold = 5;
    state.source.low_threshold = 1;
    state.source.high_threshold = 1;
    let rpc = AccountRpc {
        config: c.clone(),
        account: state.source.clone(),
        wrong_network: false,
    };
    let accounting = rpc::NativeAccounting {
        nonce: state.nonce,
        deployment_version: state.deployment_version,
        spent_daily: state.spent_daily,
        spent_total: state.spent_total,
        spent_day: state.spent_day,
        calls: state.calls,
    };
    assert!(rpc::trusted_state_for_discovery(&rpc, &old, Some(accounting.clone())).is_ok());
    assert!(rpc::trusted_state(&rpc, &old, Some(accounting.clone())).is_err());
    let live = rpc::trusted_state(&rpc, &c, Some(accounting)).unwrap();
    let p = prepare(&c, &r, &live, 86401, 86700).unwrap();
    assert!(sign(&p, &[2; 32], 86401).is_err());
    let signed = authorize(
        &p,
        &[
            sign(&p, &[4; 32], 86401).unwrap(),
            sign(&p, &[3; 32], 86401).unwrap(),
        ],
        86401,
    )
    .unwrap();
    validate_signed(&signed, 86401).unwrap();
}
#[test]
fn exact_chain_quorum_omits_unused_role_signatures_but_verifies_both_proofs() {
    for customer_weight in [1, 3] {
        let (c, r, mut s) = fixture(WalletRoute::Native, false);
        s.source.signers[0].weight = customer_weight;
        s.source.signers[1].weight = 3;
        s.source.medium_threshold = 2;
        s.source.low_threshold = 1;
        let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
        let customer = sign(&p, &[2; 32], 86401).unwrap();
        let engine = sign(&p, &[3; 32], 86401).unwrap();
        let out = authorize(&p, &[customer.clone(), engine.clone()], 86401).unwrap();
        let TransactionEnvelope::Tx(mut env) =
            TransactionEnvelope::from_xdr_base64(&out.transaction_xdr, Limits::none()).unwrap()
        else {
            panic!()
        };
        assert_eq!(env.signatures.len(), 1);
        validate_signed(&out, 86401).unwrap();
        let mut invalid = engine;
        invalid.signature[0] ^= 1;
        assert!(authorize(&p, &[customer, invalid], 86401).is_err());
        let mut extra = env.signatures.to_vec();
        extra.push(extra[0].clone());
        env.signatures = extra.try_into().unwrap();
        let mut changed = out;
        changed.transaction_xdr = env.to_xdr_base64(Limits::none()).unwrap();
        assert!(validate_signed(&changed, 86401).is_err());
    }
}
#[test]
fn additional_hash_preauthorized_and_payload_keys_do_not_lock_the_wallet() {
    let (c, r, mut s) = fixture(WalletRoute::Native, false);
    for key in [
        stellar_strkey::HashX([21; 32]).to_string(),
        stellar_strkey::PreAuthTx([22; 32]).to_string(),
        stellar_strkey::ed25519::SignedPayload {
            ed25519: [23; 32],
            payload: vec![1, 2, 3],
        }
        .to_string(),
    ] {
        s.source.signers.push(AccountSigner { key, weight: 255 });
    }
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let out = authorize(
        &p,
        &[
            sign(&p, &[2; 32], 86401).unwrap(),
            sign(&p, &[3; 32], 86401).unwrap(),
        ],
        86401,
    )
    .unwrap();
    validate_signed(&out, 86401).unwrap();
    s.source.signers.push(s.source.signers[0].clone());
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
}
#[test]
fn current_exact_preauthorization_does_not_add_unused_ed25519_signatures() {
    let (c, r, mut s) = fixture(WalletRoute::Native, false);
    let original = prepare(&c, &r, &s, 86401, 86700).unwrap();
    s.source.signers.push(AccountSigner {
        key: stellar_strkey::PreAuthTx(original.transaction_hash).to_string(),
        weight: 2,
    });
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    assert_eq!(p.transaction_hash, original.transaction_hash);
    let out = authorize(
        &p,
        &[
            sign(&p, &[2; 32], 86401).unwrap(),
            sign(&p, &[3; 32], 86401).unwrap(),
        ],
        86401,
    )
    .unwrap();
    let TransactionEnvelope::Tx(env) =
        TransactionEnvelope::from_xdr_base64(&out.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    assert!(env.signatures.is_empty());
    validate_signed(&out, 86401).unwrap();
}
#[test]
fn odc_current_agent_account_may_have_a_different_customer_signer() {
    let (mut c, r, mut s) = fixture(WalletRoute::Odc, false);
    let actual_agent = s.agent.clone();
    c.agent = account_for_seed(&[4; 32]);
    s.source.master_weight = 0;
    s.source.low_threshold = 2;
    s.source.medium_threshold = 3;
    s.source.high_threshold = 5;
    s.source.signers = vec![
        AccountSigner {
            key: c.agent.clone(),
            weight: 3,
        },
        AccountSigner {
            key: stellar_strkey::HashX([22; 32]).to_string(),
            weight: 1,
        },
    ];
    assert_eq!(s.source.address, actual_agent);
    let mut p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let resource = SorobanTransactionData {
        ext: stellar_xdr::SorobanTransactionDataExt::V0,
        resources: stellar_xdr::SorobanResources {
            footprint: stellar_xdr::LedgerFootprint {
                read_only: Default::default(),
                read_write: Default::default(),
            },
            instructions: 1,
            disk_read_bytes: 0,
            write_bytes: 0,
        },
        resource_fee: 1000,
    };
    // The fixture's admitted fee bound is high enough for this explicit resource data.
    p = attach_simulation(&p, &resource.to_xdr_base64(Limits::none()).unwrap()).unwrap();
    let out = authorize(&p, &[sign(&p, &[4; 32], 86401).unwrap()], 86401).unwrap();
    validate_signed(&out, 86401).unwrap();
    let TransactionEnvelope::Tx(env) =
        TransactionEnvelope::from_xdr_base64(&out.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        env.tx.source_account,
        stellar_xdr::MuxedAccount::Ed25519(stellar_xdr::Uint256(
            account_key(&actual_agent).unwrap()
        ))
    );
    s.source.signers[0].weight = 2;
    let underweight = prepare(&c, &r, &s, 86401, 86700).unwrap();
    assert!(
        attach_simulation(
            &underweight,
            &resource.to_xdr_base64(Limits::none()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn rpc_decodes_known_additional_signer_types_without_an_offline_allowlist() {
    let (c, _, mut state) = fixture(WalletRoute::Native, false);
    for key in [
        stellar_strkey::HashX([21; 32]).to_string(),
        stellar_strkey::PreAuthTx([22; 32]).to_string(),
        stellar_strkey::ed25519::SignedPayload {
            ed25519: [23; 32],
            payload: vec![1, 2, 3],
        }
        .to_string(),
    ] {
        state.source.signers.push(AccountSigner { key, weight: 1 });
    }
    let rpc = AccountRpc {
        config: c.clone(),
        account: state.source.clone(),
        wrong_network: false,
    };
    assert_eq!(rpc::get_account(&rpc, &c.wallet).unwrap(), state.source);
    let usage = rpc::NativeAccounting {
        nonce: 0,
        deployment_version: 0,
        spent_daily: 0,
        spent_total: 0,
        spent_day: 1,
        calls: 0,
    };
    assert!(rpc::trusted_state(&rpc, &c, Some(usage)).is_ok());
}
fn sc_map(fields: Vec<(&str, stellar_xdr::ScVal)>) -> stellar_xdr::ScVal {
    let mut entries: Vec<_> = fields
        .into_iter()
        .map(|(name, val)| stellar_xdr::ScMapEntry {
            key: stellar_xdr::ScVal::Symbol(stellar_xdr::ScSymbol(
                name.as_bytes().to_vec().try_into().unwrap(),
            )),
            val,
        })
        .collect();
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    stellar_xdr::ScVal::Map(Some(entries.try_into().unwrap()))
}
struct OdcAccountRpc {
    config: WalletConfig,
    source: AccountState,
    paused: bool,
}
impl rpc::Rpc for OdcAccountRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        use stellar_xdr::*;
        let c = &self.config;
        match method {
            "getNetwork" => Ok(json!({"passphrase":passphrase(&c.network)?})),
            "getLatestLedger" => Ok(json!({"sequence":120})),
            "getLedgerEntries" => {
                let key =
                    LedgerKey::from_xdr_base64(params["keys"][0].as_str().unwrap(), Limits::none())
                        .unwrap();
                if matches!(key, LedgerKey::Account(_)) {
                    return rpc::Rpc::call(
                        &AccountRpc {
                            config: c.clone(),
                            account: self.source.clone(),
                            wrong_network: false,
                        },
                        method,
                        params,
                    );
                }
                let state = sc_map(vec![
                    ("contract_version", ScVal::U32(1)),
                    ("paused", ScVal::Bool(self.paused)),
                    (
                        "owner",
                        ScVal::Address(address(&Contract([33; 32]).to_string())?),
                    ),
                    ("agent", ScVal::Address(address(&self.source.address)?)),
                    (
                        "engine",
                        ScVal::Address(address(&Contract([34; 32]).to_string())?),
                    ),
                    (
                        "grant",
                        sc_map(vec![("asset", ScVal::Address(address(&c.asset)?))]),
                    ),
                    ("nonce", ScVal::U64(4)),
                    ("deployment_version", ScVal::U64(2)),
                    ("spent_daily", ScVal::I128(Int128Parts { hi: 0, lo: 10 })),
                    ("spent_total", ScVal::I128(Int128Parts { hi: 0, lo: 10 })),
                    ("spent_day", ScVal::U64(1)),
                    ("calls", ScVal::U64(1)),
                ]);
                let entry = LedgerEntryData::ContractData(ContractDataEntry {
                    ext: ExtensionPoint::V0,
                    contract: address(&c.wallet)?,
                    key: ScVal::LedgerKeyContractInstance,
                    durability: ContractDataDurability::Persistent,
                    val: ScVal::ContractInstance(ScContractInstance {
                        executable: ContractExecutable::Wasm(Hash(c.module_digest)),
                        storage: Some(
                            vec![ScMapEntry {
                                key: ScVal::U32(0),
                                val: state,
                            }]
                            .try_into()
                            .unwrap(),
                        ),
                    }),
                });
                Ok(json!({"entries":[{"xdr":entry.to_xdr_base64(Limits::none()).unwrap()}]}))
            }
            "simulateTransaction" => {
                let TransactionEnvelope::Tx(tx) = TransactionEnvelope::from_xdr_base64(
                    params["transaction"].as_str().unwrap(),
                    Limits::none(),
                )
                .unwrap() else {
                    panic!()
                };
                assert_eq!(
                    tx.tx.source_account,
                    MuxedAccount::Ed25519(Uint256(account_key(&self.source.address)?))
                );
                let bytes = |b: [u8; 32]| ScVal::Bytes(ScBytes(b.to_vec().try_into().unwrap()));
                let binding = sc_map(vec![
                    ("contract_version", ScVal::U32(1)),
                    ("policy_instance", bytes(c.policy_instance)),
                    ("module_digest", bytes(c.module_digest)),
                    ("source_digest", bytes(c.source_hash)),
                    ("argument_schema_digest", bytes(hash(ARGUMENT_SCHEMA))),
                    (
                        "entrypoint",
                        ScVal::Symbol(ScSymbol(b"_execute".to_vec().try_into().unwrap())),
                    ),
                ]);
                Ok(json!({"results":[{"xdr":binding.to_xdr_base64(Limits::none()).unwrap()}]}))
            }
            _ => Err("unexpected RPC".into()),
        }
    }
}
#[test]
fn odc_discovery_reads_current_grant_source_and_does_not_lock_to_old_roles() {
    let (mut c, mut r, mut s) = fixture(WalletRoute::Odc, false);
    let obsolete = c.clone();
    c.agent = account_for_seed(&[4; 32]);
    s.source.address = account_for_seed(&[9; 32]);
    s.source.master_weight = 0;
    s.source.low_threshold = 2;
    s.source.medium_threshold = 3;
    s.source.high_threshold = 5;
    s.source.signers = vec![
        AccountSigner {
            key: c.agent.clone(),
            weight: 3,
        },
        AccountSigner {
            key: stellar_strkey::PreAuthTx([44; 32]).to_string(),
            weight: 1,
        },
    ];
    r.fee_bounds.as_mut().unwrap()[0].payer = wf(&s.source.address);
    let rpc = OdcAccountRpc {
        config: c.clone(),
        source: s.source.clone(),
        paused: false,
    };
    let discovery = rpc::trusted_state_for_discovery(&rpc, &obsolete, None).unwrap();
    assert_eq!(discovery.state.agent, s.source.address);
    assert_eq!(discovery.state.owner, Contract([33; 32]).to_string());
    assert_eq!(discovery.state.engine, Contract([34; 32]).to_string());
    assert!(rpc::trusted_state(&rpc, &obsolete, None).is_err());
    let live = rpc::trusted_state(&rpc, &c, None).unwrap();
    assert_eq!(live.source, s.source);
    assert!(prepare(&c, &r, &live, 86401, 86700).is_ok());
    r.fee_bounds.as_mut().unwrap()[0].payer = wf(&c.agent);
    assert!(prepare(&c, &r, &live, 86401, 86700).is_err());
}

#[test]
fn obsolete_engine_role_alias_is_not_an_offline_customer_key_lock() {
    let (mut c, r, mut s) = fixture(WalletRoute::Native, false);
    c.engine = c.agent.clone();
    s.engine = c.engine.clone();
    validate_config(&c).unwrap();
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err()); // One key still weighs only one, not two.
    s.source.signers[0].weight = 2;
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let proof = sign(&p, &[2; 32], 86401).unwrap();
    let out = authorize(&p, &[proof.clone(), proof], 86401).unwrap();
    validate_signed(&out, 86401).unwrap();
    let TransactionEnvelope::Tx(env) =
        TransactionEnvelope::from_xdr_base64(&out.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    assert_eq!(env.signatures.len(), 1);
}

#[test]
fn exact_preauthorization_can_complete_an_otherwise_insufficient_native_quorum() {
    let (c, r, mut s) = fixture(WalletRoute::Native, false);
    let initial = prepare(&c, &r, &s, 86401, 86700).unwrap();
    s.source.medium_threshold = 3;
    s.source.low_threshold = 3;
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
    s.source.signers.push(AccountSigner {
        key: stellar_strkey::PreAuthTx(initial.transaction_hash).to_string(),
        weight: 1,
    });
    let p = prepare(&c, &r, &s, 86401, 86700).unwrap();
    let out = authorize(
        &p,
        &[
            sign(&p, &[2; 32], 86401).unwrap(),
            sign(&p, &[3; 32], 86401).unwrap(),
        ],
        86401,
    )
    .unwrap();
    validate_signed(&out, 86401).unwrap();
    let mut wrong = r;
    wrong.operation.nonce = [31; 32];
    assert!(prepare(&c, &wrong, &s, 86401, 86700).is_err());
}
#[test]
fn paused_odc_remains_discoverable_but_cannot_authorize_execution() {
    let (c, _, s) = fixture(WalletRoute::Odc, false);
    let rpc = OdcAccountRpc {
        config: c.clone(),
        source: s.source,
        paused: true,
    };
    let discovered = rpc::trusted_state_for_discovery(&rpc, &c, None).unwrap();
    assert!(discovered.paused);
    assert_eq!(discovered.state.policy_instance, c.policy_instance);
    assert!(rpc::trusted_state(&rpc, &c, None).is_err());
}

#[test]
fn odc_exact_resource_bound_preauthorization_completes_current_customer_quorum() {
    let (c, r, mut state) = fixture(WalletRoute::Odc, false);
    let resource = SorobanTransactionData {
        resource_fee: 1000,
        ..Default::default()
    };
    let data = resource.to_xdr_base64(Limits::none()).unwrap();
    let base = prepare(&c, &r, &state, 86401, 86700).unwrap();
    let final_hash = attach_simulation(&base, &data).unwrap().transaction_hash;
    state.source.low_threshold = 2;
    state.source.medium_threshold = 2;
    let insufficient = prepare(&c, &r, &state, 86401, 86700).unwrap();
    assert!(attach_simulation(&insufficient, &data).is_err());
    state.source.signers.push(AccountSigner {
        key: stellar_strkey::PreAuthTx(final_hash).to_string(),
        weight: 1,
    });
    let before_simulation = prepare(&c, &r, &state, 86401, 86700).unwrap();
    assert!(sign(&before_simulation, &[2; 32], 86401).is_err());
    let prepared = attach_simulation(&before_simulation, &data).unwrap();
    assert_eq!(prepared.transaction_hash, final_hash);
    let signed = authorize(
        &prepared,
        &[sign(&prepared, &[2; 32], 86401).unwrap()],
        86401,
    )
    .unwrap();
    validate_signed(&signed, 86401).unwrap();
    let altered = SorobanTransactionData {
        resource_fee: 1001,
        ..Default::default()
    }
    .to_xdr_base64(Limits::none())
    .unwrap();
    assert!(attach_simulation(&before_simulation, &altered).is_err());
}

#[test]
fn retained_packet_validation_does_not_recompile_source_or_grant_execution() {
    let (c, r, state) = fixture(WalletRoute::Native, false);
    let prepared = prepare(&c, &r, &state, 86401, 86700).unwrap();
    let mut signed = sign_native(&prepared);
    // Model source metadata from a retired compiler namespace. The already signed
    // native body and effects stay byte-for-byte identical; this is proof, not admission.
    signed.prepared.config.policy_source="use allowit::v1::prelude::*; async fn _execute(ctx:&Context)->PolicyResult { retired_vendor::check(ctx)?; Ok(()) }".into();
    signed.prepared.config.source_hash = hash(&signed.prepared.config.policy_source);
    signed.prepared.state.source_hash = signed.prepared.config.source_hash;
    let original = serde_json::to_vec(&signed).unwrap();
    validate_retained_prepared(&signed.prepared).unwrap();
    validate_retained_signed(&signed).unwrap();
    assert!(validate_prepared(&signed.prepared, 86401).is_err());
    assert!(sign(&signed.prepared, &[2; 32], 86401).is_err());
    assert!(validate_signed(&signed, 86401).is_err());
    let rpc = FakeRpc {
        calls: Mutex::new(vec![]),
        signed: signed.clone(),
        fail_send: false,
    };
    assert!(rpc::submit(&rpc, &mut Store::default(), &signed, 86401).is_err());
    assert!(rpc.calls.lock().unwrap().is_empty());
    let mut store = Store {
        value: Some((signed.clone(), OperationStatus::SubmissionUnknown)),
    };
    assert_eq!(
        rpc::submit(&rpc, &mut store, &signed, 90000).unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(*rpc.calls.lock().unwrap(), vec!["getTransaction"]);
    assert_eq!(
        serde_json::to_vec(&store.value.as_ref().unwrap().0).unwrap(),
        original
    );
    assert_eq!(
        rpc::recover(&rpc, &mut store, r.operation.nonce).unwrap(),
        OperationStatus::Confirmed
    );
    assert_eq!(*rpc.calls.lock().unwrap(), vec!["getTransaction"]);
    let mut changed = signed.clone();
    changed.prepared.config.policy_source.push(' ');
    assert!(validate_retained_signed(&changed).is_err());
    let mut changed = signed;
    changed.transaction_hash[0] ^= 1;
    assert!(validate_retained_signed(&changed).is_err());
}

struct AdminRpc {
    base: OdcAccountRpc,
    state_contract_version: u32,
    binding_contract_version: Option<stellar_xdr::ScVal>,
    calls: Mutex<Vec<(String, Vec<stellar_xdr::ScVal>)>>,
    balance: i128,
    wrong_owner: bool,
    bad_auth: bool,
}
fn replace_field(value: &mut stellar_xdr::ScVal, name: &str, new: stellar_xdr::ScVal) {
    let stellar_xdr::ScVal::Map(Some(map)) = value else {
        panic!()
    };
    let mut fields = map.to_vec();
    fields.retain(
        |e| !matches!(&e.key,stellar_xdr::ScVal::Symbol(s) if s.0.as_slice()==name.as_bytes()),
    );
    fields.push(stellar_xdr::ScMapEntry {
        key: stellar_xdr::ScVal::Symbol(stellar_xdr::ScSymbol(
            name.as_bytes().to_vec().try_into().unwrap(),
        )),
        val: new,
    });
    fields.sort_by(|a, b| a.key.cmp(&b.key));
    *map = fields.try_into().unwrap();
}
impl rpc::Rpc for AdminRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        use stellar_xdr::*;
        if method == "getLedgerEntries" {
            let key =
                LedgerKey::from_xdr_base64(params["keys"][0].as_str().unwrap(), Limits::none())
                    .unwrap();
            if matches!(key, LedgerKey::Account(_)) {
                let account = AccountState {
                    address: self.base.config.owner.clone(),
                    sequence: 40,
                    master_weight: 1,
                    low_threshold: 1,
                    medium_threshold: 1,
                    high_threshold: 1,
                    signers: vec![],
                };
                // Trusted delegated discovery reads the agent; owner administration reads owner.
                let LedgerKey::Account(a) = &key else {
                    unreachable!()
                };
                if a.account_id
                    != AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(account_key(
                        &account.address,
                    )?)))
                {
                    return self.base.call(method, params);
                }
                return AccountRpc {
                    config: self.base.config.clone(),
                    account,
                    wrong_network: false,
                }
                .call(method, params);
            }
            let mut response = self.base.call(method, params)?;
            let mut entry = LedgerEntryData::from_xdr_base64(
                response["entries"][0]["xdr"].as_str().unwrap(),
                Limits::none(),
            )
            .unwrap();
            let LedgerEntryData::ContractData(ref mut data) = entry else {
                panic!()
            };
            let ScVal::ContractInstance(ref mut instance) = data.val else {
                panic!()
            };
            let mut storage = instance.storage.clone().unwrap().to_vec();
            let state = &mut storage[0].val;
            replace_field(
                state,
                "contract_version",
                ScVal::U32(self.state_contract_version),
            );
            replace_field(
                state,
                "owner",
                ScVal::Address(address(if self.wrong_owner {
                    &self.base.source.address
                } else {
                    &self.base.config.owner
                })?),
            );
            instance.storage = Some(storage.try_into().unwrap());
            response["entries"][0]["xdr"] = json!(entry.to_xdr_base64(Limits::none()).unwrap());
            return Ok(response);
        }
        if method == "simulateTransaction" {
            let TransactionEnvelope::Tx(envelope) = TransactionEnvelope::from_xdr_base64(
                params["transaction"].as_str().unwrap(),
                Limits::none(),
            )
            .unwrap() else {
                panic!()
            };
            let OperationBody::InvokeHostFunction(op) = &envelope.tx.operations[0].body else {
                panic!()
            };
            let HostFunction::InvokeContract(call) = &op.host_function else {
                panic!()
            };
            let function = String::from_utf8(call.function_name.0.to_vec()).unwrap();
            if function == "policy_binding" {
                let bytes = |v: [u8; 32]| ScVal::Bytes(ScBytes(v.to_vec().try_into().unwrap()));
                let c = &self.base.config;
                let mut fields = vec![
                    ("policy_instance", bytes(c.policy_instance)),
                    ("module_digest", bytes(c.module_digest)),
                    ("source_digest", bytes(c.source_hash)),
                    ("argument_schema_digest", bytes(hash(ARGUMENT_SCHEMA))),
                    (
                        "entrypoint",
                        ScVal::Symbol(ScSymbol(b"_execute".to_vec().try_into().unwrap())),
                    ),
                ];
                if let Some(version) = &self.binding_contract_version {
                    fields.push(("contract_version", version.clone()));
                }
                return Ok(
                    json!({"results":[{"xdr":sc_map(fields).to_xdr_base64(Limits::none()).unwrap()}]}),
                );
            }
            self.calls
                .lock()
                .unwrap()
                .push((function.clone(), call.args.to_vec()));
            if function == "balance" {
                return Ok(
                    json!({"results":[{"xdr":ScVal::I128(Int128Parts{hi:(self.balance>>64) as i64,lo:self.balance as u64}).to_xdr_base64(Limits::none()).unwrap()}]}),
                );
            }
            let mut auth = op.auth.to_vec();
            if function == "deploy_policy" {
                let ScVal::Map(Some(fields)) = &call.args[0] else {
                    panic!()
                };
                let owner = fields
                    .iter()
                    .find(|e| {
                        e.key == ScVal::Symbol(ScSymbol(b"owner".to_vec().try_into().unwrap()))
                    })
                    .unwrap();
                let ScVal::Address(next_owner) = &owner.val else {
                    panic!()
                };
                if address_text(next_owner)? != self.base.config.owner {
                    let mut extra = auth[0].clone();
                    extra.credentials = SorobanCredentials::Address(SorobanAddressCredentials {
                        address: next_owner.clone(),
                        nonce: 7,
                        signature_expiration_ledger: 200,
                        signature: ScVal::Void,
                    });
                    auth.push(extra);
                }
            }
            if self.bad_auth {
                auth[0].root_invocation.sub_invocations =
                    vec![auth[0].root_invocation.clone()].try_into().unwrap();
            }
            return Ok(
                json!({"minResourceFee":"1000","transactionData":SorobanTransactionData{resource_fee:1000,..Default::default()}.to_xdr_base64(Limits::none()).unwrap(),"results":[{"xdr":ScVal::Void.to_xdr_base64(Limits::none()).unwrap(),"auth":auth.iter().map(|a|a.to_xdr_base64(Limits::none()).unwrap()).collect::<Vec<_>>()}]}),
            );
        }
        self.base.call(method, params)
    }
}
fn admin_fixture(
    state_contract_version: u32,
    binding_contract_version: Option<stellar_xdr::ScVal>,
) -> AdminRpc {
    let (config, _, state) = fixture(WalletRoute::Odc, false);
    AdminRpc {
        base: OdcAccountRpc {
            config,
            source: state.source,
            paused: true,
        },
        state_contract_version,
        binding_contract_version,
        calls: Mutex::new(vec![]),
        balance: 1234567,
        wrong_owner: false,
        bad_auth: false,
    }
}
#[test]
fn owner_admin_delegates_full_balance_and_versions_without_agent_or_engine_calls() {
    use allowit_policy_sdk::admin::{Admin, DeploymentVersion};
    use allowit_stellar_wallet::admin::OwnerAdmin;
    let rpc = admin_fixture(1, Some(stellar_xdr::ScVal::U32(1)));
    let c = &rpc.base.config;
    let mut facade = Admin::new(OwnerAdmin {
        rpc: &rpc,
        config: c,
        now: 86401,
    });
    let recovered = facade.recover_funds(&c.asset).unwrap();
    assert_eq!(recovered.function, "recover");
    assert_eq!(recovered.deployment_version, 2);
    let calls = rpc.calls.lock().unwrap();
    let (_, args) = calls.last().unwrap();
    assert_eq!(args.len(), 2);
    assert_eq!(
        args[0],
        stellar_xdr::ScVal::Address(address(&c.owner).unwrap())
    );
    assert_eq!(
        args[1],
        stellar_xdr::ScVal::I128(stellar_xdr::Int128Parts { hi: 0, lo: 1234567 })
    );
    drop(calls);
    assert!(facade.recover_funds(&c.wallet).is_err());
    assert!(facade.set_pause(false, DeploymentVersion(2)).is_err());
    assert!(facade.set_pause(false, DeploymentVersion(4)).is_err());
    let paused = facade.set_pause(false, DeploymentVersion(3)).unwrap();
    assert_eq!(paused.function, "set_pause");
    assert_eq!(paused.deployment_version, 3);
    let TransactionEnvelope::Tx(envelope) =
        TransactionEnvelope::from_xdr_base64(&paused.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    assert!(envelope.signatures.is_empty());
    assert_eq!(envelope.tx.fee, 1100);
    assert_eq!(paused.required_authorizers, vec![c.owner.clone()]);
}
#[test]
fn owner_admin_fails_closed_on_release_owner_mismatch_auth_and_empty_balance() {
    use allowit_policy_sdk::admin::{Admin, DeploymentVersion};
    use allowit_stellar_wallet::admin::OwnerAdmin;
    for case in ["release", "owner", "auth", "balance"] {
        let mut rpc = admin_fixture(1, Some(stellar_xdr::ScVal::U32(1)));
        match case {
            "release" => {
                rpc.state_contract_version = 0;
                rpc.binding_contract_version = None
            }
            "owner" => rpc.wrong_owner = true,
            "auth" => rpc.bad_auth = true,
            "balance" => rpc.balance = 0,
            _ => unreachable!(),
        }
        let mut admin = Admin::new(OwnerAdmin {
            rpc: &rpc,
            config: &rpc.base.config,
            now: 86401,
        });
        assert!(
            admin.recover_funds(&rpc.base.config.asset).is_err(),
            "{case}"
        );
        if case != "balance" {
            assert!(
                admin.set_pause(true, DeploymentVersion(3)).is_err(),
                "{case}"
            );
        }
    }
}
#[test]
fn contract_release_and_persisted_counter_names_are_exact() {
    use stellar_xdr::ScVal;
    for (state, binding, works) in [
        (1, Some(ScVal::U32(1)), true),
        (0, Some(ScVal::U32(1)), false),
        (2, Some(ScVal::U32(1)), false),
        (1, None, false),
        (1, Some(ScVal::U32(2)), false),
        (1, Some(ScVal::Bool(true)), false),
    ] {
        let rpc = admin_fixture(state, binding);
        let observed = rpc::trusted_state_for_discovery(&rpc, &rpc.base.config, None);
        assert_eq!(observed.is_ok(), works);
        if let Ok(observed) = observed {
            assert_eq!(observed.state.deployment_version, 2);
            assert_eq!(observed.state.contract_version, Some(1));
        }
    }
    let (_, _, mut old) = fixture(WalletRoute::Odc, false);
    old.contract_version = None;
    let mut legacy = serde_json::to_value(&old).unwrap();
    let version = legacy
        .as_object_mut()
        .unwrap()
        .remove("deploymentVersion")
        .unwrap();
    legacy["revision"] = version;
    assert_eq!(
        serde_json::from_value::<TrustedState>(legacy.clone()).unwrap(),
        old
    );
    legacy["deploymentVersion"] = json!(4);
    assert!(serde_json::from_value::<TrustedState>(legacy).is_err());
    for unshipped in [
        "interfaceVersion",
        "contractInterfaceVersion",
        "policyVersion",
        "policyDeploymentVersion",
    ] {
        let mut invalid = serde_json::to_value(&old).unwrap();
        invalid[unshipped] = json!(1);
        assert!(serde_json::from_value::<TrustedState>(invalid).is_err());
    }
}

#[test]
fn full_policy_deployment_uses_one_versioned_owner_flow_and_owner_handoff_authorization() {
    use allowit_policy_sdk::admin::{Admin, DeploymentVersion};
    use allowit_stellar_wallet::admin::{Grant, OwnerAdmin, PolicyDeployment};
    for enabled in [false, true] {
        let rpc = admin_fixture(1, Some(stellar_xdr::ScVal::U32(1)));
        let c = &rpc.base.config;
        let next_owner = account_for_seed(&[51; 32]);
        let artifact = PolicyDeployment {
            owner: next_owner.clone(),
            agent: c.agent.clone(),
            engine: c.engine.clone(),
            grant: Grant {
                asset: c.asset.clone(),
                recipients: vec![c.owner.clone()],
                per_transfer: 10,
                daily_limit: 20,
                total_limit: 30,
                max_calls: 4,
                expires_at: 86700,
            },
            paused: true,
            x402_enabled: enabled,
            wasm_hash: Some([8; 32]),
        };
        let mut admin = Admin::new(OwnerAdmin {
            rpc: &rpc,
            config: c,
            now: 86401,
        });
        assert!(
            admin
                .deploy_policy(DeploymentVersion(2), artifact.clone())
                .is_err()
        );
        assert!(
            admin
                .deploy_policy(DeploymentVersion(4), artifact.clone())
                .is_err()
        );
        let prepared = admin.deploy_policy(DeploymentVersion(3), artifact).unwrap();
        assert_eq!(
            prepared.required_authorizers,
            vec![c.owner.clone(), next_owner]
        );
        let calls = rpc.calls.lock().unwrap();
        let (name, args) = calls.last().unwrap();
        assert_eq!(name, "deploy_policy");
        assert_eq!(args.len(), 1);
        let stellar_xdr::ScVal::Map(Some(fields)) = &args[0] else {
            panic!()
        };
        assert_eq!(fields.len(), 8);
        assert!(fields.iter().any(|e|matches!(&e.key,stellar_xdr::ScVal::Symbol(k) if k.0.as_slice()==b"deployment_version") && e.val==stellar_xdr::ScVal::U64(3)));
        assert!(fields.iter().any(
            |e| matches!(&e.key,stellar_xdr::ScVal::Symbol(k) if k.0.as_slice()==b"x402_enabled")
                && e.val == stellar_xdr::ScVal::Bool(enabled)
        ));
        assert!(!fields.iter().any(|e| matches!(&e.key,stellar_xdr::ScVal::Symbol(k) if [b"x402_signer".as_slice(),b"policy_version".as_slice()].contains(&k.0.as_slice()))));
    }
}
#[test]
fn canonical_main_revision_packet_is_status_only_and_preserves_signed_bytes() {
    use ed25519_dalek::{Signer, SigningKey};
    use stellar_xdr::*;
    let (c, r, state) = fixture(WalletRoute::Odc, false);
    let mut p = attach_simulation(
        &prepare(&c, &r, &state, 86401, 86700).unwrap(),
        &SorobanTransactionData::default()
            .to_xdr_base64(Limits::none())
            .unwrap(),
    )
    .unwrap();
    let TransactionEnvelope::Tx(mut env) =
        TransactionEnvelope::from_xdr_base64(&p.transaction_xdr, Limits::none()).unwrap()
    else {
        panic!()
    };
    let mut ops = env.tx.operations.to_vec();
    let OperationBody::InvokeHostFunction(op) = &mut ops[0].body else {
        panic!()
    };
    let HostFunction::InvokeContract(call) = &mut op.host_function else {
        panic!()
    };
    let mut args = call.args.to_vec();
    let ScVal::Map(Some(fields)) = &mut args[0] else {
        panic!()
    };
    let mut entries = fields.to_vec();
    for f in &mut entries {
        if matches!(&f.key,ScVal::Symbol(k) if k.0.as_slice()==b"deployment_version") {
            f.key = ScVal::Symbol(ScSymbol(b"revision".to_vec().try_into().unwrap()));
        }
    }
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    *fields = entries.try_into().unwrap();
    call.args = args.try_into().unwrap();
    let mut auth = op.auth.to_vec();
    auth[0].root_invocation.function = SorobanAuthorizedFunction::ContractFn(call.clone());
    op.auth = auth.try_into().unwrap();
    env.tx.operations = ops.try_into().unwrap();
    p.state.contract_version = None;
    p.transaction_hash = hash(
        TransactionSignaturePayload {
            network_id: Hash(network_id(&c.network).unwrap()),
            tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(env.tx.clone()),
        }
        .to_xdr(Limits::none())
        .unwrap(),
    );
    p.transaction_xdr = TransactionEnvelope::Tx(env.clone())
        .to_xdr_base64(Limits::none())
        .unwrap();
    let key = SigningKey::from_bytes(&[2; 32]);
    env.signatures = vec![DecoratedSignature {
        hint: SignatureHint(account_key(&c.agent).unwrap()[28..].try_into().unwrap()),
        signature: stellar_xdr::Signature(
            key.sign(&p.transaction_hash)
                .to_bytes()
                .to_vec()
                .try_into()
                .unwrap(),
        ),
    }]
    .try_into()
    .unwrap();
    let signed = SignedOperation {
        transaction_hash: p.transaction_hash,
        prepared: p,
        transaction_xdr: TransactionEnvelope::Tx(env)
            .to_xdr_base64(Limits::none())
            .unwrap(),
    };
    validate_retained_signed(&signed).unwrap();
    assert!(validate_signed(&signed, 86401).is_err());
    assert!(prepare(&c, &r, &signed.prepared.state, 86401, 86700).is_err());
    assert!(sign(&signed.prepared, &[2; 32], 86401).is_err());
    let rpc = FakeRpc {
        calls: Mutex::new(vec![]),
        signed: signed.clone(),
        fail_send: false,
    };
    let receipt = rpc::receipt_by_hash(&rpc, &c.network, &signed.transaction_hash)
        .unwrap()
        .unwrap();
    rpc::validate_receipt(&receipt, &c.network, &signed.transaction_hash).unwrap();
    let mut changed = signed;
    changed.prepared.state.contract_version = Some(1);
    assert!(validate_retained_signed(&changed).is_err());
}

#[test]
fn enabled_x402_rejects_contract_agents() {
    use allowit_policy_sdk::admin::{Admin, DeploymentVersion};
    use allowit_stellar_wallet::admin::{Grant, OwnerAdmin, PolicyDeployment};
    let rpc = admin_fixture(1, Some(stellar_xdr::ScVal::U32(1)));
    let c = &rpc.base.config;
    let mut admin = Admin::new(OwnerAdmin {
        rpc: &rpc,
        config: c,
        now: 86401,
    });
    let artifact = PolicyDeployment {
        owner: c.owner.clone(),
        agent: c.wallet.clone(),
        engine: c.engine.clone(),
        grant: Grant {
            asset: c.asset.clone(),
            recipients: vec![c.owner.clone()],
            per_transfer: 1,
            daily_limit: 2,
            total_limit: 3,
            max_calls: 4,
            expires_at: 86700,
        },
        paused: true,
        x402_enabled: true,
        wasm_hash: None,
    };
    assert!(admin.deploy_policy(DeploymentVersion(3), artifact).is_err());
    assert!(rpc.calls.lock().unwrap().is_empty());
}

#[test]
fn explicit_engine_release_query_distinguishes_legacy_absence_from_failures() {
    use stellar_xdr::*;
    struct MetadataRpc {
        response: serde_json::Value,
        network: &'static str,
    }
    impl rpc::Rpc for MetadataRpc {
        fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
            if method == "getNetwork" {
                return Ok(json!({"passphrase": self.network}));
            }
            assert_eq!(method, "simulateTransaction");
            let TransactionEnvelope::Tx(env) = TransactionEnvelope::from_xdr_base64(
                params["transaction"].as_str().unwrap(),
                Limits::none(),
            )
            .unwrap() else {
                panic!()
            };
            let OperationBody::InvokeHostFunction(op) = &env.tx.operations[0].body else {
                panic!()
            };
            let HostFunction::InvokeContract(call) = &op.host_function else {
                panic!()
            };
            assert_eq!(
                call.contract_address,
                address(&Contract([3; 32]).to_string()).unwrap()
            );
            assert_eq!(call.function_name.0.as_slice(), b"engine_version");
            assert!(call.args.is_empty() && op.auth.is_empty() && env.signatures.is_empty());
            Ok(self.response.clone())
        }
    }
    let (c, _, state) = fixture(WalletRoute::Odc, false);
    let invoke = |response| {
        rpc::engine_version(
            &MetadataRpc {
                response,
                network: "Test SDF Network ; September 2015",
            },
            &c,
            &state.source,
        )
    };
    for (value, expected) in [
        (ScVal::U32(1), Ok(Some(1))),
        (ScVal::U32(7), Ok(Some(7))),
        (ScVal::U32(0), Err(())),
        (ScVal::U64(1), Err(())),
        (ScVal::Bool(true), Err(())),
    ] {
        assert_eq!(
            invoke(json!({"results":[{"xdr":value.to_xdr_base64(Limits::none()).unwrap()}]}))
                .map_err(|_| ()),
            expected
        );
    }
    let event = DiagnosticEvent {
        in_successful_contract_call: false,
        event: ContractEvent {
            ext: ExtensionPoint::V0,
            contract_id: Some(ContractId(Hash([3; 32]))),
            type_: ContractEventType::Diagnostic,
            body: ContractEventBody::V0(ContractEventV0 {
                topics: vec![
                    ScVal::Symbol(ScSymbol(b"error".to_vec().try_into().unwrap())),
                    ScVal::Error(ScError::WasmVm(ScErrorCode::MissingValue)),
                ]
                .try_into()
                .unwrap(),
                data: ScVal::Vec(Some(
                    vec![
                        ScVal::String(ScString(
                            b"trying to invoke non-existent contract function"
                                .to_vec()
                                .try_into()
                                .unwrap(),
                        )),
                        ScVal::Symbol(ScSymbol(b"engine_version".to_vec().try_into().unwrap())),
                    ]
                    .try_into()
                    .unwrap(),
                )),
            }),
        },
    };
    let missing = |event: &DiagnosticEvent| json!({"error":"HostError: Error(WasmVm, MissingValue)","events":[event.to_xdr_base64(Limits::none()).unwrap()]});
    assert_eq!(invoke(missing(&event)).unwrap(), None);
    let mut wrong_contract = event.clone();
    wrong_contract.event.contract_id = Some(ContractId(Hash([9; 32])));
    assert!(invoke(missing(&wrong_contract)).is_err());
    let mut wrong_function = event.clone();
    let ContractEventBody::V0(body) = &mut wrong_function.event.body;
    body.data = ScVal::Void;
    assert!(invoke(missing(&wrong_function)).is_err());
    for response in [
        json!({"error":"HostError: Error(WasmVm, MissingValue)"}),
        json!({"error":"network unavailable"}),
        json!({"error":"HostError: Error(WasmVm, MissingValue)","events":["bad-xdr"]}),
        json!({"results":[]}),
    ] {
        assert!(invoke(response).is_err());
    }
    assert!(
        rpc::engine_version(
            &MetadataRpc {
                response: json!({}),
                network: "wrong network"
            },
            &c,
            &state.source
        )
        .is_err()
    );
}
