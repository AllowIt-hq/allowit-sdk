//! Generic payment terms produced by an authenticated service adapter.
//! Vendor challenge decoding and delivery verification belong to that adapter.
use crate::{address, hex, unhex, units, Result, ZERO};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServicePaymentRequest {
    pub version: u8,
    pub method: String,
    pub url: String,
    pub origin: String,
    /// Digest of the complete original request verified by the service adapter.
    pub request_digest: String,
    pub chain_id: u64,
    pub asset: String,
    pub recipient: String,
    pub amount_units: String,
    pub memo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

/// Check normalized payment terms. This does not decode a vendor challenge or prove delivery.
pub fn validate_service_payment_request(
    request: &ServicePaymentRequest,
    chain_id: u64,
    asset: &str,
    recipient: &str,
    amount: &str,
    now: u64,
) -> Result<()> {
    if request.version != 1
        || request.chain_id != chain_id
        || request.method.is_empty()
        || request.method.len() > 32
        || !request
            .method
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'-')
        || request.url.len() > 2048
        || request.url.chars().any(char::is_control)
    {
        return Err("Invalid normalized service request".into());
    }
    let url = Url::parse(&request.url).map_err(|_| "Invalid service URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.as_str() != request.url
        || url.origin().ascii_serialization() != request.origin
    {
        return Err("Service request requires an exact HTTPS URL and origin".into());
    }
    let asset = address(asset)?;
    let recipient = address(recipient)?;
    if asset == ZERO
        || recipient == ZERO
        || address(&request.asset)? != asset
        || address(&request.recipient)? != recipient
        || request.amount_units != units(amount)?.to_string()
        || request.amount_units == "0"
    {
        return Err("Service request changed the exact asset, recipient or amount".into());
    }
    for value in [&request.request_digest, &request.memo] {
        let bytes = unhex(value)?;
        if bytes.len() != 32 || hex(&bytes) != *value {
            return Err("Service request requires canonical bytes32 bindings".into());
        }
    }
    if request.expires_at.is_some_and(|expiry| expiry <= now) {
        return Err("Service payment terms have expired".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> ServicePaymentRequest {
        ServicePaymentRequest {
            version: 1,
            method: "POST".into(),
            url: "https://merchant.example/paid?q=exact".into(),
            origin: "https://merchant.example".into(),
            request_digest: hex(&[25; 32]),
            chain_id: 42431,
            asset: hex(&[13; 20]),
            recipient: hex(&[19; 20]),
            amount_units: "250000".into(),
            memo: hex(&[26; 32]),
            expires_at: Some(300),
        }
    }
    fn check(request: &ServicePaymentRequest) -> Result<()> {
        validate_service_payment_request(
            request,
            42431,
            &hex(&[13; 20]),
            &hex(&[19; 20]),
            "0.25",
            100,
        )
    }
    #[test]
    fn normalized_terms_bind_chain_asset_recipient_amount_and_deadline() {
        let request = fixture();
        assert!(check(&request).is_ok());
        for field in [
            "version",
            "chainId",
            "asset",
            "recipient",
            "amountUnits",
            "expiresAt",
        ] {
            let mut value = serde_json::to_value(&request).unwrap();
            value[field] = match field {
                "version" => 2.into(),
                "chainId" => 31337.into(),
                "expiresAt" => 100.into(),
                "amountUnits" => "0250000".into(),
                _ => hex(&[88; 20]).into(),
            };
            assert!(
                check(&serde_json::from_value(value).unwrap()).is_err(),
                "{field}"
            );
        }
        for field in ["requestDigest", "memo"] {
            let mut value = serde_json::to_value(&request).unwrap();
            value[field] = hex(&[77; 31]).into();
            assert!(check(&serde_json::from_value(value).unwrap()).is_err());
        }
    }
    #[test]
    fn normalized_origin_url_and_method_are_exact() {
        for url in [
            "http://merchant.example/paid",
            "https://user:secret@merchant.example/paid",
            "https://merchant.example/paid#fragment",
        ] {
            let mut request = fixture();
            request.url = url.into();
            assert!(check(&request).is_err());
        }
        let mut request = fixture();
        request.origin = "https://other.example".into();
        assert!(check(&request).is_err());
        request = fixture();
        request.method = "GET\r\nCookie".into();
        assert!(check(&request).is_err());
        request = fixture();
        request.expires_at = None;
        assert!(check(&request).is_ok());
    }
}
