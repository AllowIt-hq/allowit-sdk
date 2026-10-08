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
        "25f8336a9bd38a60e45329d909fcd1c915bff6fc4c2efd446253da9e66c07873"
    );
    assert_eq!(inspected["operationId"], "06".repeat(32));
    assert_eq!(inspected["executed"], false);
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
    request.expires_slot = request.signing_slot;
    assert!(dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())})).is_err());
    request.expires_slot += 1;
    request.action = Action::PayUsdc { amount: 0 };
    assert!(dispatch(&json!({"operation":"paysh::inspect_request", "requestBase64":STANDARD.encode(borsh::to_vec(&request).unwrap())})).is_err());
}
