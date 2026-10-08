use allowit_contract_client::dispatch;
use allowit_paysh_interface::{Action, Request};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

#[test]
fn focused_policy_compiler_retains_bound_namespaced_source() {
    let source = "pub async fn execute(ctx: &Context) -> PolicyResult { allowit::set_cap(ctx, \"5\", \"USDC\")?; Ok(()) }";
    let compiled = dispatch(&json!({"operation":"allowit::compile_policy", "source":source, "originalIntent":"Small purchases"})).unwrap();
    assert_eq!(
        compiled["sourceHash"],
        allowit_sdk::digest(source.as_bytes())
    );
    assert!(compiled["compiledIR"].to_string().contains("set_cap"));
    assert!(!compiled["compiledIR"].to_string().contains("allowit::"));
    assert!(dispatch(&json!({"operation":"paysh::compile_policy", "source":source})).is_err());
    assert!(dispatch(&json!({"operation":"allowit::compile_policy", "op":"compile"})).is_err());
}
fn request() -> Request {
    Request {
        network: [1; 32],
        program: [2; 32],
        policy: [3; 32],
        owner: [4; 32],
        module_digest: [5; 32],
        operation_id: [6; 32],
        nonce: [7; 32],
        challenge_hash: [8; 32],
        evidence_hash: [9; 32],
        signing_slot: 1000,
        signing_timestamp: 3601,
        expires_slot: 1180,
        expires_timestamp: 3661,
        service_fee_lamports: 1000,
        action: Action::PayUsdc { amount: 1_000_000 },
    }
}
#[test]
fn paysh_inspection_uses_concrete_request_bytes_and_identity() {
    let request = request();
    let bytes = borsh::to_vec(&request).unwrap();
    let input =
        json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(&bytes)});
    let inspected = dispatch(&input).unwrap();
    assert_eq!(inspected["action"]["operation"], "paysh::pay_usdc");
    assert_eq!(
        inspected["requestHash"],
        "471619df8b715557596e009e83a1e96324b7842cf39ae5338fc165e5e5009549"
    );
    assert_eq!(inspected["operationId"], "06".repeat(32));
    assert_eq!(inspected["executed"], false);
    assert_eq!(inspected["serviceFeeLamports"], "1000");
    assert_eq!(inspected["signingSlot"], "1000");
    assert_eq!(inspected["signingTimestamp"], "3601");
    assert_eq!(
        inspected["requestBytesSha256"],
        "25f8336a9bd38a60e45329d909fcd1c915bff6fc4c2efd446253da9e66c07873"
    );
    let mut substituted = request.clone();
    substituted.challenge_hash[0] ^= 1;
    let other = dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&substituted).unwrap())})).unwrap();
    assert_ne!(other["requestHash"], inspected["requestHash"]);
    let mut extra = bytes;
    extra.push(0);
    assert!(
        dispatch(
            &json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(extra)})
        )
        .is_err()
    );
}
#[test]
fn paysh_inspection_rejects_invalid_action_and_expiry() {
    let mut request = request();
    request.expires_slot = request.signing_slot - 1;
    assert!(dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())})).is_err());
    request.expires_slot += 1;
    request.action = Action::PayUsdc { amount: 0 };
    assert!(dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())})).is_err());
}

#[test]
fn concrete_swap_inspection_preserves_integer_width_and_signing_message() {
    let mut request = request();
    request.action = Action::SwapSolToUsdc {
        amount_in_lamports: 9_007_199_254_740_993,
        min_out_usdc: u64::MAX,
        sqrt_price_limit: u128::MAX,
        tick_arrays: [[10; 32], [11; 32], [12; 32]],
    };
    let input = json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())});
    let result = dispatch(&input).unwrap();
    assert_eq!(result["action"]["operation"], "paysh::swap_sol_to_usdc");
    assert_eq!(result["action"]["amountInLamports"], "9007199254740993");
    assert_eq!(result["action"]["minOutUsdc"], u64::MAX.to_string());
    assert_eq!(result["action"]["sqrtPriceLimit"], u128::MAX.to_string());
    assert_eq!(
        result["signedMessageBase64"],
        STANDARD.encode(request.signed_message())
    );
    assert!(
        dispatch(
            &json!({"operation":"stripe::inspect_request", "requestBase64":input["requestBase64"]})
        )
        .is_err()
    );
}

#[test]
fn paysh_inspection_matches_native_expiry_equality_and_negative_time_rules() {
    let mut request = request();
    request.expires_slot = request.signing_slot;
    request.expires_timestamp = request.signing_timestamp;
    assert!(dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())})).is_ok());
    request.signing_timestamp = -1;
    assert!(dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())})).is_err());
}
fn activation() -> serde_json::Value {
    let key = |n| solana_program::pubkey::Pubkey::new_from_array([n; 32]).to_string();
    json!({"operation":"solana::prepare_activation",
        "source":"pub async fn execute(ctx: &Context) -> PolicyResult { allowit::set_cap(ctx, \"5\", \"USDC\")?; Ok(()) }",
        "originalIntent":"Small purchases", "policyId":"01".repeat(32),
        "programId":key(9),"stateAddress":key(8),"owner":key(1),"executor":key(2),"compiler":key(3),"recipient":key(4),
        "revision":"1", "expiresAt":"2000", "allocationUnits":"5000000"})
}
#[test]
fn devnet_demo_binds_the_explicit_network_and_canonical_asset() {
    let result = dispatch(&activation()).unwrap();
    assert_eq!(result["mandate"]["network"], "solana:devnet");
    assert_eq!(
        result["mandate"]["asset"],
        "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
    );
}
#[test]
fn present_wrong_field_types_never_change_mandate_bindings_to_defaults() {
    let input = activation();
    let result = dispatch(&input).unwrap();
    assert_eq!(result["mandate"]["action"], "transfer");
    assert_eq!(result["mandate"]["merchant"], "demo");
    assert_eq!(result["mandate"]["compilerKeyId"], "demo-compiler");
    for field in ["action", "merchant", "compilerKeyId"] {
        for wrong in [json!(null), json!(false), json!(12), json!([])] {
            let mut invalid = input.clone();
            invalid[field] = wrong;
            assert!(dispatch(&invalid).is_err(), "{field}");
        }
    }
    for wrong in [
        json!(null),
        json!(false),
        json!("480"),
        json!(480.5),
        json!(-1),
        json!(u64::MAX),
    ] {
        let mut invalid = input.clone();
        invalid["uploadChunkBytes"] = wrong;
        assert!(dispatch(&invalid).is_err());
    }
}
