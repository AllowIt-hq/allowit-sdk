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
        source: account,
        nonce: 9,
        revision: 4,
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
fn fourth_signer_and_wrong_thresholds_fail_closed() {
    let (c, r, mut s) = fixture(WalletRoute::Native, false);
    s.source.signers.push(AccountSigner {
        key: account_for_seed(&[12; 32]),
        weight: 1,
    });
    assert!(prepare(&c, &r, &s, 86401, 86700).is_err());
    s.source.signers.pop();
    s.source.high_threshold = 2;
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
    assert_eq!(call.function_name.0.as_slice(), b"execute");
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
                                key: stellar_xdr::SignerKey::Ed25519(stellar_xdr::Uint256(
                                    account_key(&s.key)?,
                                )),
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
        revision: 4,
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
    assert!(rpc::receipt_by_hash(&rpc, "stellar:mainnet", &signed.transaction_hash).is_err());
    assert!(rpc::receipt_by_hash(&rpc, &c.network, &[0; 32]).is_err());
}
