use super::*;

fn fixture() -> Value {
    // Public deterministic seeds and mock bytecode; these fixtures are never deployed.
    let vault_code = b"allowit-vault-fixture-not-deployable";
    let policy_code = b"allowit-policy-fixture-not-deployable";
    let c = Config {
        network: "stellar:testnet".into(),
        rpc_url: "https://soroban-testnet.stellar.org".into(),
        vault: Contract([7; 32]).to_string(),
        owner: account_for_seed(&[1; 32]),
        executor: account_for_seed(&[2; 32]),
        authority: account_for_seed(&[4; 32]),
        asset: USDC_TESTNET.into(),
        policy: Contract([5; 32]).to_string(),
        vault_wasm_hash: hex(&sha256(vault_code)),
        policy_wasm_hash: hex(&sha256(policy_code)),
        policy_source_hash: hex(&[9; 32]),
    };
    let now = 172800;
    let s = VaultState {
        owner: c.owner.clone(),
        executor: c.executor.clone(),
        authority: c.authority.clone(),
        asset: c.asset.clone(),
        policy: c.policy.clone(),
        policy_source: c.policy_source_hash.clone(),
        policy_wasm: c.policy_wasm_hash.clone(),
        daily_limit: 100_000,
        spent: 100,
        spent_day: now / 86400,
        nonce: 3,
        revision: 4,
        approved: true,
    };
    let offer = json!({"scheme":"exact","network":c.network,"asset":c.asset,"amount":"10000","payTo":account_for_seed(&[3;32]),"maxTimeoutSeconds":300,"extra":{"areFeesSponsored":true}});
    let resource = json!({"url":"https://agent402.tools/api/hash","mimeType":"application/json"});
    let required = json!({"x402Version":2,"resource":resource,"accepts":[offer]});
    let request = PaymentRequest {
        method: "GET".into(),
        url: "https://agent402.tools/api/hash?text=hi".into(),
        body: vec![],
        resource: resource.clone(),
        payment_required: required.clone(),
        selected_offer: offer.clone(),
    };
    let root = SorobanAuthorizedInvocation {
        function: SorobanAuthorizedFunction::ContractFn(transfer_args(&c, &offer).unwrap()),
        sub_invocations: Default::default(),
    };
    let auth = SorobanAuthorizationEntry {
        credentials: SorobanCredentials::Address(SorobanAddressCredentials {
            address: sc_address(&c.vault).unwrap(),
            nonce: 12345,
            signature_expiration_ledger: 0,
            signature: ScVal::Void,
        }),
        root_invocation: root,
    };
    let auth_xdr = write_xdr(&auth).unwrap();
    let mut p = prepare_payment(&c, &s, request.clone(), &auth_xdr, 3456, now + 100, now).unwrap();
    let data = write_xdr(&SorobanTransactionData::default()).unwrap();
    p.transaction_xdr = attach_simulation_data(&p.transaction_xdr, &data, "1000").unwrap();
    let approval = authority_sign(&c, &p, &[4; 32]).unwrap();
    let signed = sign_payment(&c, &p, &[2; 32], &approval).unwrap();
    let state_value = map(vec![
        ("approved", ScVal::Bool(s.approved)),
        ("asset", ScVal::Address(sc_address(&s.asset).unwrap())),
        (
            "authority",
            ScVal::Address(sc_address(&s.authority).unwrap()),
        ),
        ("daily_limit", ScVal::U64(s.daily_limit)),
        ("executor", ScVal::Address(sc_address(&s.executor).unwrap())),
        ("nonce", ScVal::U64(s.nonce)),
        ("owner", ScVal::Address(sc_address(&s.owner).unwrap())),
        ("policy", ScVal::Address(sc_address(&s.policy).unwrap())),
        ("policy_source", bytes(&[9; 32]).unwrap()),
        (
            "policy_wasm",
            bytes(&unhex::<32>(&s.policy_wasm).unwrap()).unwrap(),
        ),
        ("revision", ScVal::U64(s.revision)),
        ("spent", ScVal::U64(s.spent)),
        ("spent_day", ScVal::U64(s.spent_day)),
    ])
    .unwrap();
    let entries=[
        LedgerEntryData::ContractData(ContractDataEntry {ext:ExtensionPoint::V0,contract:sc_address(&c.vault).unwrap(),key:ScVal::LedgerKeyContractInstance,durability:ContractDataDurability::Persistent,val:ScVal::ContractInstance(ScContractInstance {executable:ContractExecutable::Wasm(Hash(unhex::<32>(&c.vault_wasm_hash).unwrap())),storage:None})}),
        LedgerEntryData::ContractData(ContractDataEntry {ext:ExtensionPoint::V0,contract:sc_address(&c.policy).unwrap(),key:ScVal::LedgerKeyContractInstance,durability:ContractDataDurability::Persistent,val:ScVal::ContractInstance(ScContractInstance {executable:ContractExecutable::Wasm(Hash(unhex::<32>(&c.policy_wasm_hash).unwrap())),storage:None})}),
        LedgerEntryData::ContractCode(ContractCodeEntry {ext:ContractCodeEntryExt::V0,hash:Hash(sha256(vault_code)),code:vault_code.to_vec().try_into().unwrap()}),
        LedgerEntryData::ContractCode(ContractCodeEntry {ext:ContractCodeEntryExt::V0,hash:Hash(sha256(policy_code)),code:policy_code.to_vec().try_into().unwrap()}),
    ].iter().map(|d|json!({"key":write_xdr(&d.to_key()).unwrap(),"xdr":write_xdr(d).unwrap(),"lastModifiedLedgerSeq":3400})).collect::<Vec<_>>();
    let mut confirmed: TransactionEnvelope = read_xdr(&signed.transaction_xdr).unwrap();
    let tx = tx_mut(&mut confirmed).unwrap();
    tx.tx.source_account =
        MuxedAccount::Ed25519(Uint256(account_key(&account_for_seed(&[6; 32])).unwrap()));
    tx.tx.seq_num = SequenceNumber(50);
    tx.tx.fee = 2000;
    let tx_hash = hex(&confirmed
        .hash(sha256(network_passphrase(&c.network).unwrap()))
        .unwrap());
    let event = ContractEvent {
        ext: ExtensionPoint::V0,
        contract_id: Some(ContractId(Hash(contract_hash(&c.asset).unwrap()))),
        type_: ContractEventType::Contract,
        body: ContractEventBody::V0(ContractEventV0 {
            topics: vec![
                symbol("transfer").unwrap(),
                ScVal::Address(sc_address(&c.vault).unwrap()),
                ScVal::Address(sc_address(text(&offer, "payTo").unwrap()).unwrap()),
            ]
            .try_into()
            .unwrap(),
            data: ScVal::I128(Int128Parts { hi: 0, lo: 10000 }),
        }),
    };
    let result = TransactionResult {
        fee_charged: 2000,
        result: TransactionResultResult::TxSuccess(Default::default()),
        ext: TransactionResultExt::V0,
    };
    let meta = TransactionMeta::V4(TransactionMetaV4 {
        operations: vec![OperationMetaV2 {
            events: vec![event].try_into().unwrap(),
            ..Default::default()
        }]
        .try_into()
        .unwrap(),
        ..Default::default()
    });
    let receipt = SettlementReceipt {
        transaction: tx_hash.clone(),
        payer: c.vault.clone(),
        network: c.network.clone(),
    };
    let receipt_value =
        json!({"success":true,"transaction":tx_hash,"payer":c.vault,"network":c.network});
    let rpc = json!({"status":"SUCCESS","txHash":tx_hash,"ledger":3446,"createdAt":"172850","envelopeXdr":write_xdr(&confirmed).unwrap(),"resultXdr":write_xdr(&result).unwrap(),"resultMetaXdr":write_xdr(&meta).unwrap()});
    json!({"fixtureOnly":true,"config":c,"state":s,"request":request,"paymentRequired":required,"selectedOffer":offer,"authorizationXdr":auth_xdr,"stateResultXdr":write_xdr(&state_value).unwrap(),"transactionData":data,"minResourceFee":"1000","prepared":p,"approval":approval,"signed":signed,"ledgerEntries":entries,"receipt":receipt,"paymentResponse":STANDARD.encode(serde_json::to_vec(&receipt_value).unwrap()),"rpcTransaction":rpc})
}

fn objects() -> (Config, PreparedPayment, DetachedApproval, SignedPayment) {
    let fixture = fixture();
    (
        serde_json::from_value(fixture["config"].clone()).unwrap(),
        serde_json::from_value(fixture["prepared"].clone()).unwrap(),
        serde_json::from_value(fixture["approval"].clone()).unwrap(),
        serde_json::from_value(fixture["signed"].clone()).unwrap(),
    )
}

#[test]
fn separate_authority_and_executor_signatures_are_required() {
    let (c, p, a, s) = objects();
    validate_signed(&c, &p, &s).unwrap();
    assert!(authority_sign(&c, &p, &[2; 32]).is_err());
    assert!(sign_payment(&c, &p, &[4; 32], &a).is_err());
    let mut wrong = a;
    wrong.signature = hex(&[0; 64]);
    assert!(sign_payment(&c, &p, &[2; 32], &wrong).is_err());
}

#[test]
fn plan_mutation_cannot_reuse_detached_approval() {
    let (c, p, a, _) = objects();
    let mut revised = p.clone();
    revised.state.revision += 1;
    assert!(verify_approval(&c, &revised, &a).is_err());
    let mut nonce = p.clone();
    nonce.state.nonce += 1;
    assert!(verify_approval(&c, &nonce, &a).is_err());
    let mut request = p.clone();
    request.request.url.push_str("&another=1");
    assert!(verify_approval(&c, &request, &a).is_err());
    let mut revoked = p.clone();
    revoked.state.approved = false;
    assert!(verify_approval(&c, &revoked, &a).is_err());
    assert!(validate_prepared(&c, &p, p.expires_at).is_err());
}

#[test]
fn saved_header_and_signed_metadata_cannot_be_edited() {
    let (c, p, _, mut s) = objects();
    s.payload["accepted"]["amount"] = json!("10010");
    assert!(validate_signed(&c, &p, &s).is_err());
    let (_, _, _, mut s) = objects();
    s.payment_signature = STANDARD.encode(b"{}");
    assert!(validate_signed(&c, &p, &s).is_err());
}

#[test]
fn excessive_resource_fee_is_rejected_before_signing() {
    let (c, mut p, _, _) = objects();
    let data = write_xdr(&SorobanTransactionData::default()).unwrap();
    assert!(attach_simulation_data(&p.transaction_xdr, &data, "49900").is_ok());
    assert!(attach_simulation_data(&p.transaction_xdr, &data, "49901").is_err());
    assert!(attach_simulation_data(&p.transaction_xdr, &data, "-1").is_err());
    let mut tx: TransactionEnvelope = read_xdr(&p.transaction_xdr).unwrap();
    tx_mut(&mut tx).unwrap().tx.fee = 50001;
    p.transaction_xdr = write_xdr(&tx).unwrap();
    assert!(authority_sign(&c, &p, &[4; 32]).is_err());
}

#[test]
fn simulation_auth_cannot_add_an_operation_or_another_payer() {
    let (c, p, _, _) = objects();
    let mut auth: SorobanAuthorizationEntry = read_xdr(&p.authorization_xdr).unwrap();
    auth.root_invocation.sub_invocations = vec![auth.root_invocation.clone()].try_into().unwrap();
    assert!(prepare_payment(
        &c,
        &p.state,
        p.request.clone(),
        &write_xdr(&auth).unwrap(),
        3456,
        p.expires_at,
        p.created_at
    )
    .is_err());
    auth.root_invocation.sub_invocations = Default::default();
    credentials_mut(&mut auth).unwrap().address = sc_address(&c.executor).unwrap();
    assert!(prepare_payment(
        &c,
        &p.state,
        p.request.clone(),
        &write_xdr(&auth).unwrap(),
        3456,
        p.expires_at,
        p.created_at
    )
    .is_err());
}

#[test]
fn live_contract_pins_and_state_schema_are_verified() {
    let f = fixture();
    let c: Config = serde_json::from_value(f["config"].clone()).unwrap();
    let s: VaultState = serde_json::from_value(f["state"].clone()).unwrap();
    let mut entries = f["ledgerEntries"].as_array().unwrap().clone();
    validate_contract_entries(&c, &entries).unwrap();
    assert_eq!(
        parse_vault_state(f["stateResultXdr"].as_str().unwrap()).unwrap(),
        s
    );
    entries.pop();
    assert!(validate_contract_entries(&c, &entries).is_err());
    let mut wrong = c;
    wrong.vault_wasm_hash = hex(&[1; 32]);
    assert!(validate_contract_entries(&wrong, f["ledgerEntries"].as_array().unwrap()).is_err());
}

#[test]
fn receipt_matches_rewritten_envelope_and_exact_event_after_expiry() {
    let f = fixture();
    let (c, p, _, s) = objects();
    let receipt = parse_payment_response(f["paymentResponse"].as_str().unwrap(), &c).unwrap();
    validate_receipt(&c, &p, &s, &receipt, &f["rpcTransaction"]).unwrap();
    let mut wrong = f["rpcTransaction"].clone();
    wrong["txHash"] = json!(hex(&[1; 32]));
    assert!(validate_receipt(&c, &p, &s, &receipt, &wrong).is_err());
    let mut meta: TransactionMeta =
        read_xdr(f["rpcTransaction"]["resultMetaXdr"].as_str().unwrap()).unwrap();
    if let TransactionMeta::V4(meta) = &mut meta {
        let event = meta
            .operations
            .iter_mut()
            .next()
            .unwrap()
            .events
            .iter_mut()
            .next()
            .unwrap();
        let ContractEventBody::V0(body) = &mut event.body;
        body.data = ScVal::I128(Int128Parts { hi: 0, lo: 10010 });
    }
    let mut wrong = f["rpcTransaction"].clone();
    wrong["resultMetaXdr"] = json!(write_xdr(&meta).unwrap());
    assert!(validate_receipt(&c, &p, &s, &receipt, &wrong).is_err());
    let mut wrong = f["rpcTransaction"].clone();
    wrong["status"] = json!("NOT_FOUND");
    assert!(validate_receipt(&c, &p, &s, &receipt, &wrong).is_err());
}

#[test]
fn write_and_verify_cli_fixture() {
    let f = fixture();
    if std::env::var_os("ALLOWIT_WRITE_FIXTURE").is_some() {
        std::fs::create_dir_all("tests/fixtures").unwrap();
        std::fs::write(
            "tests/fixtures/x402-vault.json",
            serde_json::to_vec_pretty(&f).unwrap(),
        )
        .unwrap();
    }
    let stored: Value = if std::env::var_os("ALLOWIT_WRITE_FIXTURE").is_some() {
        serde_json::from_slice(&std::fs::read("tests/fixtures/x402-vault.json").unwrap()).unwrap()
    } else {
        serde_json::from_str(include_str!("../tests/fixtures/x402-vault.json")).unwrap()
    };
    assert_eq!(f, stored);
}
