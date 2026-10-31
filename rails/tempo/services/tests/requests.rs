use allowit_sdk::service::rpc::Operation;
use serde_json::json;
#[test]
fn managed_tempo_wire_and_verification_classification_remain_vendor_owned() {
    use allowit_tempo_services::rpc as tempo;
    let request =
        tempo::Request::estimate_managed(4217, "owner", "contract", "calldata", "token", 15);
    assert_eq!(request.method(), "eth_estimateGas");
    assert_eq!(
        request.params(),
        json!([{"type":"0x76","chainId":"0x1079","from":"owner","calls":[{"to":"contract","data":"calldata","value":"0x0"}],"feeToken":"token","nonceKey":"0x0","nonce":"0xf","keyType":"secp256k1"}])
    );
    assert!(request.is_verification());
    assert!(!tempo::Request::send_raw("signed-packet").is_verification());
}
