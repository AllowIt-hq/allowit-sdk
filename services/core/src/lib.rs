//! Service execution types shared by the SDK and concrete vendor packages.
use serde_json::Value;
use std::{collections::BTreeMap, fmt};
pub mod rpc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
pub fn fault(status: u16, message: impl Into<String>) -> Error {
    Error {
        status,
        code: "",
        message: message.into(),
    }
}
pub fn coded(status: u16, code: &'static str, message: impl Into<String>) -> Error {
    Error {
        status,
        code,
        message: message.into(),
    }
}
pub fn hash(value: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(value.as_ref()))
}
pub fn bad(message: impl Into<String>) -> Error {
    fault(400, message)
}

/// Deployment configuration. It must not be serialized into a service request.
#[derive(Clone, Default)]
pub struct Environment(pub BTreeMap<String, String>);
impl Environment {
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn present(&self, key: &str) -> bool {
        !self.get(key).is_empty()
    }
}

/// Public, bound operation data. This type carries no wallet or vendor secret.
pub struct Invocation<'a> {
    pub service_id: &'a str,
    pub network: &'a str,
    pub operation_id: &'a str,
    pub input: &'a Value,
}

pub trait Upstream: Send + Sync {
    fn validate_input(&self, input: &Value) -> Result<()>;
    fn invoke(&self, context: &Invocation<'_>) -> Result<Value>;
    /// Return an existing response without another provider request.
    fn cached(&self, _: &Invocation<'_>) -> Result<Option<Value>> {
        Ok(None)
    }
    /// Resume only through an implementation's explicit recovery contract.
    fn resume(&self, context: &Invocation<'_>) -> Result<Option<Value>> {
        self.cached(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_commitment_hashes_exact_input_bytes() {
        assert_eq!(
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(hash(b"abc"), hash(b"abc\n"));
    }
    #[test]
    fn machine_code_and_http_status_are_distinct() {
        let error = coded(409, "operation_pending", "Retain the original operation.");
        assert_eq!(error.status, 409);
        assert_eq!(error.code, "operation_pending");
        assert_eq!(error.to_string(), "Retain the original operation.");
        assert_eq!(bad("Invalid input.").code, "");
    }
}
