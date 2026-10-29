use serde_json::Value;
/// A vendor-owned JSON-RPC operation. The caller supplies transport policy.
pub trait Operation {
    fn method(&self) -> &str;
    fn params(&self) -> Value;
}
