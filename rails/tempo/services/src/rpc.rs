use allowit_sdk::service::rpc::Operation;
use serde_json::{Value, json};
pub struct Request {
    method: &'static str,
    params: Value,
}
impl Operation for Request {
    fn method(&self) -> &str {
        self.method
    }
    fn params(&self) -> Value {
        self.params.clone()
    }
}
impl Request {
    pub fn is_verification(&self) -> bool {
        self.method != "eth_sendRawTransaction"
    }
    pub fn call_from(from: &str, to: &str, data: &str, value: &str, tag: &str) -> Self {
        Self::new(
            "eth_call",
            json!([{"from":from,"to":to,"data":data,"value":value},tag]),
        )
    }
    pub fn estimate_gas_fields(
        from: &str,
        to: &str,
        data: &str,
        value: &str,
        fee_token: Option<&str>,
    ) -> Self {
        let mut transaction = json!({"from":from,"to":to,"data":data,"value":value});
        if let Some(token) = fee_token {
            transaction["feeToken"] = json!(token);
        }
        Self::new("eth_estimateGas", json!([transaction]))
    }
    pub fn estimate_managed(
        chain_id: u64,
        from: &str,
        to: &str,
        data: &str,
        fee_token: &str,
        nonce: u64,
    ) -> Self {
        Self::new(
            "eth_estimateGas",
            json!([{"type":"0x76","chainId":format!("0x{chain_id:x}"),"from":from,"calls":[{"to":to,"data":data,"value":"0x0"}],"feeToken":fee_token,"nonceKey":"0x0","nonce":format!("0x{nonce:x}"),"keyType":"secp256k1"}]),
        )
    }
    pub fn block_number(number: u64, full: bool) -> Self {
        Self::block(&format!("0x{number:x}"), full)
    }
    fn new(method: &'static str, params: Value) -> Self {
        Self { method, params }
    }
    pub fn chain_id() -> Self {
        Self::new("eth_chainId", json!([]))
    }
    pub fn code(address: &str, tag: &str) -> Self {
        Self::new("eth_getCode", json!([address, tag]))
    }
    pub fn call(address: &str, data: &str, tag: &str) -> Self {
        Self::new("eth_call", json!([{"to":address,"data":data},tag]))
    }
    pub fn block(tag: &str, full: bool) -> Self {
        Self::new("eth_getBlockByNumber", json!([tag, full]))
    }
    pub fn transaction(hash: &str) -> Self {
        Self::new("eth_getTransactionByHash", json!([hash]))
    }
    pub fn receipt(hash: &str) -> Self {
        Self::new("eth_getTransactionReceipt", json!([hash]))
    }
    pub fn nonce(address: &str, tag: &str) -> Self {
        Self::new("eth_getTransactionCount", json!([address, tag]))
    }
    pub fn estimate_gas(transaction: Value) -> Self {
        Self::new("eth_estimateGas", json!([transaction]))
    }
    pub fn gas_price() -> Self {
        Self::new("eth_gasPrice", json!([]))
    }
    pub fn send_raw(wire: &str) -> Self {
        Self::new("eth_sendRawTransaction", json!([wire]))
    }
}

/// Preserve the rail's existing temporary-read refusal classification.
pub fn is_temporary_read_refusal(error: &allowit_sdk::transport::rpc::Error) -> bool {
    error.is_well_formed_refusal() && matches!(error.rpc_code, Some(-32002 | -32005))
}
