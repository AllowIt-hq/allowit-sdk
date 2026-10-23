use allowit_stellar::http_binding::*;
use allowit_stellar::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use stellar_xdr::*;

fn original() -> Value {
    serde_json::from_str(include_str!("fixtures/x402-vault.json")).unwrap()
}

fn prepared(
    request: PaymentRequest,
    v2: bool,
) -> (Config, VaultState, PreparedPayment, SignedPayment) {
    let f = original();
    let c: Config = serde_json::from_value(f["config"].clone()).unwrap();
    let s: VaultState = serde_json::from_value(f["state"].clone()).unwrap();
    let mut entry = SorobanAuthorizationEntry::from_xdr_base64(
        f["prepared"]["authorizationXdr"].as_str().unwrap(),
        Limits::none(),
    )
    .unwrap();
    if v2 {
        if let SorobanCredentials::Address(credentials) = entry.credentials {
            entry.credentials = SorobanCredentials::AddressV2(credentials);
        }
    }
    let mut p = prepare_payment(
        &c,
        &s,
        request,
        &entry.to_xdr_base64(Limits::none()).unwrap(),
        3456,
        172900,
        172800,
    )
    .unwrap();
    p.transaction_xdr = attach_simulation_data(
        &p.transaction_xdr,
        f["transactionData"].as_str().unwrap(),
        "1000",
    )
    .unwrap();
    let approval = authority_sign(&c, &p, &[4; 32]).unwrap();
    let signed = sign_payment(&c, &p, &[2; 32], &approval).unwrap();
    (c, s, p, signed)
}

fn request() -> PaymentRequest {
    let mut r: PaymentRequest =
        serde_json::from_value(original()["prepared"]["request"].clone()).unwrap();
    r.selected_offer["extra"]["allowitHttpBinding"] = json!({
        "profile": HTTP_BINDING_PROFILE,
        "quoteId": "11".repeat(32), "issuedAt":172800,"expiresAt":173000,
        "merchant":"fixture-merchant","action":"hash",
        "verificationKey":account_for_seed(&[8;32]),"verifierHash":"22".repeat(32)
    });
    r.selected_offer["extra"]["paymentFlow"] = json!("upfront");
    r.payment_required["accepts"][0] = r.selected_offer.clone();
    r
}

fn actual(r: &PaymentRequest) -> HttpRequest {
    HttpRequest {
        method: r.method.clone(),
        url: r.url.clone(),
        body: r.body.clone(),
    }
}

fn verify(
    c: &Config,
    s: &VaultState,
    r: &PaymentRequest,
    a: &HttpRequest,
    credential: &str,
) -> allowit_stellar::Result<PaymentAuthorization> {
    verify_http_payment(c, s, r, a, credential, 172800, 3444)
}

fn envelope_change(
    signed: &SignedPayment,
    mutate: impl FnOnce(&mut TransactionV1Envelope),
) -> String {
    let mut envelope =
        TransactionEnvelope::from_xdr_base64(&signed.transaction_xdr, Limits::none()).unwrap();
    let TransactionEnvelope::Tx(tx) = &mut envelope else {
        panic!("fixture envelope")
    };
    mutate(tx);
    let mut payload = signed.payload.clone();
    payload["payload"]["transaction"] = json!(envelope.to_xdr_base64(Limits::none()).unwrap());
    STANDARD.encode(serde_json::to_vec(&payload).unwrap())
}

#[test]
fn merchant_rejects_unchanged_credential_on_different_resource() {
    let (c, s, p, signed) = prepared(request(), false);
    let mut a = actual(&p.request);
    a.url = "https://agent402.tools/api/blocked?text=hi".into();
    assert!(verify(&c, &s, &p.request, &a, &signed.payment_signature).is_err());
}

#[test]
fn both_credential_arms_verify_against_current_public_state() {
    for v2 in [false, true] {
        let (c, s, p, signed) = prepared(request(), v2);
        let result = verify(
            &c,
            &s,
            &p.request,
            &actual(&p.request),
            &signed.payment_signature,
        )
        .unwrap();
        assert_eq!(result.vault, c.vault);
        assert_eq!(result.request_hash, p.request_hash);
        assert_eq!(result.nonce, 3);
        assert_eq!(result.revision, 4);
        assert_eq!(result.expires_at, 172900);
        assert_eq!(result.signature_expiration_ledger, 3456);
        assert_eq!(
            inspect_payment_authorization(&signed.payment_signature).unwrap(),
            result
        );
    }
}

#[test]
fn actual_method_query_and_raw_body_cannot_change() {
    let mut r = request();
    r.method = "POST".into();
    r.body = br#"{"text":"hi"}"#.to_vec();
    let (c, s, p, signed) = prepared(r, false);
    let a = actual(&p.request);
    assert!(verify(&c, &s, &p.request, &a, &signed.payment_signature).is_ok());
    let mut changed = a.clone();
    changed.method = "PUT".into();
    assert!(verify(&c, &s, &p.request, &changed, &signed.payment_signature).is_err());
    changed = a.clone();
    changed.url = "https://agent402.tools/api/hash?text=other".into();
    assert!(verify(&c, &s, &p.request, &changed, &signed.payment_signature).is_err());
    changed = a;
    changed.body = br#"{"text": "hi"}"#.to_vec();
    assert!(verify(&c, &s, &p.request, &changed, &signed.payment_signature).is_err());
}

#[test]
fn original_complete_challenge_and_unsigned_payload_metadata_are_checked() {
    let (c, s, p, signed) = prepared(request(), false);
    let a = actual(&p.request);
    let mut r = p.request.clone();
    r.payment_required["error"] = json!("different challenge");
    assert!(verify(&c, &s, &r, &a, &signed.payment_signature).is_err());
    for field in ["resource", "accepted"] {
        let mut payload = signed.payload.clone();
        if field == "resource" {
            payload[field]["description"] = json!("other action");
        } else {
            payload[field]["extra"]["injected"] = json!(true);
        }
        let credential = STANDARD.encode(serde_json::to_vec(&payload).unwrap());
        assert!(verify(&c, &s, &p.request, &a, &credential).is_err());
    }
}

#[test]
fn a_forged_matching_request_hash_does_not_reuse_the_signatures() {
    let (c, s, p, signed) = prepared(request(), false);
    let mut r = p.request.clone();
    r.url = "https://agent402.tools/api/hash?text=other".into();
    let forged = request_hash(&r).unwrap();
    let credential = envelope_change(&signed, |tx| {
        let OperationBody::InvokeHostFunction(op) =
            &mut tx.tx.operations.iter_mut().next().unwrap().body
        else {
            panic!("operation")
        };
        let SorobanCredentials::Address(credentials) =
            &mut op.auth.iter_mut().next().unwrap().credentials
        else {
            panic!("credentials")
        };
        let ScVal::Map(Some(fields)) = &mut credentials.signature else {
            panic!("signature")
        };
        for field in fields.0.iter_mut() {
            if field.key == ScVal::Symbol(ScSymbol(b"request_hash".to_vec().try_into().unwrap())) {
                field.val = ScVal::Bytes(ScBytes(forged.to_vec().try_into().unwrap()));
            }
        }
    });
    assert_eq!(
        inspect_payment_authorization(&credential)
            .unwrap()
            .request_hash,
        hex(&forged)
    );
    assert!(verify(&c, &s, &r, &actual(&r), &credential).is_err());
}

#[test]
fn stale_state_approval_budget_and_expiration_fail_closed() {
    let (c, s, p, signed) = prepared(request(), false);
    let a = actual(&p.request);
    for changed in [
        VaultState {
            nonce: 4,
            ..s.clone()
        },
        VaultState {
            revision: 5,
            ..s.clone()
        },
        VaultState {
            approved: false,
            ..s.clone()
        },
        VaultState {
            daily_limit: 1099,
            ..s.clone()
        },
        VaultState {
            spent_day: 3,
            ..s.clone()
        },
        VaultState {
            executor: account_for_seed(&[9; 32]),
            ..s.clone()
        },
    ] {
        assert!(verify(&c, &changed, &p.request, &a, &signed.payment_signature).is_err());
    }
    for (now, ledger) in [
        (172900, 3444),
        (172800, 3456),
        (172800, 0),
        (172800, 3395),
        (172799, 3444),
    ] {
        assert!(verify_http_payment(
            &c,
            &s,
            &p.request,
            &a,
            &signed.payment_signature,
            now,
            ledger
        )
        .is_err());
    }
}

#[test]
fn merchant_quote_profile_is_mandatory_without_changing_saved_journals() {
    let f = original();
    let c: Config = serde_json::from_value(f["config"].clone()).unwrap();
    let p: PreparedPayment = serde_json::from_value(f["prepared"].clone()).unwrap();
    let signed: SignedPayment = serde_json::from_value(f["signed"].clone()).unwrap();
    assert!(validate_signed(&c, &p, &signed).is_ok());
    assert!(verify(
        &c,
        &p.state,
        &p.request,
        &actual(&p.request),
        &signed.payment_signature
    )
    .is_err());
    for (field, value) in [
        ("profile", json!("unsupported")),
        ("expiresAt", json!(172800)),
        ("issuedAt", json!(172801)),
        ("quoteId", json!("short")),
        ("verificationKey", json!("not-a-key")),
        ("verifierHash", json!("ff".repeat(33))),
        ("expiresAt", json!(173401)),
        ("unknown", json!(true)),
    ] {
        let mut r = request();
        r.selected_offer["extra"]["allowitHttpBinding"][field] = value;
        r.payment_required["accepts"][0] = r.selected_offer.clone();
        let (c, s, p, signed) = prepared(r, false);
        assert!(verify(
            &c,
            &s,
            &p.request,
            &actual(&p.request),
            &signed.payment_signature
        )
        .is_err());
    }
}

#[test]
fn sponsor_envelope_rewrites_preserve_authorization_but_extra_operations_do_not() {
    let (c, s, p, signed) = prepared(request(), false);
    let rewritten = envelope_change(&signed, |tx| {
        tx.tx.source_account = MuxedAccount::Ed25519(Uint256([8; 32]));
        tx.tx.seq_num = SequenceNumber(50);
        tx.tx.fee = 2000;
        tx.tx.memo = Memo::Text(b"sponsor".to_vec().try_into().unwrap());
    });
    assert!(verify(&c, &s, &p.request, &actual(&p.request), &rewritten).is_ok());
    let extra = envelope_change(&signed, |tx| {
        tx.tx.operations = vec![tx.tx.operations[0].clone(), tx.tx.operations[0].clone()]
            .try_into()
            .unwrap();
    });
    assert!(inspect_payment_authorization(&extra).is_err());
    let too_expensive = envelope_change(&signed, |tx| {
        tx.tx.fee = 50001;
    });
    assert!(verify(&c, &s, &p.request, &actual(&p.request), &too_expensive).is_err());
}

#[test]
fn malformed_authorization_and_added_invocations_are_rejected() {
    let (_, _, _, signed) = prepared(request(), false);
    let malformed = envelope_change(&signed, |tx| {
        let OperationBody::InvokeHostFunction(op) =
            &mut tx.tx.operations.iter_mut().next().unwrap().body
        else {
            panic!("operation")
        };
        let SorobanCredentials::Address(credentials) =
            &mut op.auth.iter_mut().next().unwrap().credentials
        else {
            panic!("credentials")
        };
        credentials.signature = ScVal::Void;
    });
    assert!(inspect_payment_authorization(&malformed).is_err());
    let nested = envelope_change(&signed, |tx| {
        let OperationBody::InvokeHostFunction(op) =
            &mut tx.tx.operations.iter_mut().next().unwrap().body
        else {
            panic!("operation")
        };
        let auth = op.auth.iter_mut().next().unwrap();
        auth.root_invocation.sub_invocations =
            vec![auth.root_invocation.clone()].try_into().unwrap();
    });
    assert!(inspect_payment_authorization(&nested).is_err());
    let duplicate = envelope_change(&signed, |tx| {
        let OperationBody::InvokeHostFunction(op) =
            &mut tx.tx.operations.iter_mut().next().unwrap().body
        else {
            panic!("operation")
        };
        op.auth = vec![op.auth[0].clone(), op.auth[0].clone()]
            .try_into()
            .unwrap();
    });
    assert!(inspect_payment_authorization(&duplicate).is_err());
    assert!(inspect_payment_authorization(&"A".repeat(2_097_153)).is_err());
}

fn claims() -> AttestationClaims {
    AttestationClaims {
        profile: HTTP_BINDING_PROFILE.into(),
        merchant: "fixture-merchant".into(),
        action: "hash".into(),
        origin: "https://agent402.tools".into(),
        request_hash: "33".repeat(32),
        challenge_hash: "44".repeat(32),
        nonce: "55".repeat(32),
        issued_at: 172800,
        expires_at: 172830,
        verifier_hash: "22".repeat(32),
    }
}

#[test]
fn merchant_attestation_verifies_the_claims_with_a_trusted_public_key() {
    let claims = claims();
    let signature = hex(&SigningKey::from_bytes(&[8; 32])
        .sign(&attestation_digest(&claims).unwrap())
        .to_bytes());
    let key = account_for_seed(&[8; 32]);
    assert!(verify_merchant_attestation(&claims, &signature, &key).is_ok());
    let mut changed = claims.clone();
    changed.action = "other-action".into();
    assert!(verify_merchant_attestation(&changed, &signature, &key).is_err());
    assert!(verify_merchant_attestation(&claims, &signature, &account_for_seed(&[9; 32])).is_err());
    assert!(verify_merchant_attestation(&claims, "00", &key).is_err());
    changed = claims.clone();
    changed.expires_at = 172861;
    assert!(attestation_digest(&changed).is_err());
    changed = claims.clone();
    changed.origin = "https://agent402.tools/path".into();
    assert!(attestation_digest(&changed).is_err());
    changed = claims;
    changed.verifier_hash = "AA".repeat(32);
    assert!(attestation_digest(&changed).is_err());
    let mut unknown = serde_json::to_value(changed).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AttestationClaims>(unknown).is_err());
}
