//! Offline verification of a deliberately narrow MPP Tempo charge/push purchase.
//! This is request binding, not proof of an HTTP response or merchant delivery.
use crate::{address, hex, unhex, units, Result, ZERO};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MppPaymentRequest {
    pub version: u8,
    pub method: String,
    pub url: String,
    /// The entire original decoded challenge, including its encoded request and extensions.
    pub challenge: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MppPurchase {
    pub protocol: String,
    pub method: String,
    pub url: String,
    pub origin: String,
    pub request_digest: String,
    pub challenge_digest: String,
    pub chain_id: u64,
    pub token: String,
    pub recipient: String,
    pub amount_units: String,
    pub memo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge_expires_at: Option<u64>,
}

fn sha256(value: &Value) -> Result<String> {
    Ok(hex(&Sha256::digest(crate::canonical_json(value)?)))
}

fn text<'a>(object: &'a Value, field: &str, max: usize) -> Result<&'a str> {
    let value = object[field].as_str().ok_or("Invalid MPP text field")?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err("Invalid MPP text field".into());
    }
    Ok(value)
}

/// mppx Attribution v1 with no optional client attribution: tag, version, realm,
/// zero client field and challenge ID. This never derives a memo from the local request ID.
pub fn mpp_memo(challenge: &Value) -> Result<String> {
    let realm = text(challenge, "realm", 512)?;
    let id = text(challenge, "id", 512)?;
    let mut memo = [0u8; 32];
    memo[..4].copy_from_slice(&crate::keccak("mpp")[..4]);
    memo[4] = 1;
    memo[5..15].copy_from_slice(&crate::keccak(realm)[..10]);
    memo[25..].copy_from_slice(&crate::keccak(id)[..7]);
    Ok(hex(&memo))
}

/// The MVP accepts only public HTTPS GET URLs with no query, credentials or fragment.
/// Both participants bind the original spelling; normalization must not redirect a purchase.
pub fn mpp_url(value: &str) -> Result<(String, String)> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err("Invalid MPP URL".into());
    }
    let url = Url::parse(value).map_err(|_| "Invalid MPP URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.as_str() != value
    {
        return Err(
            "MPP supports exact HTTPS GET URLs without query, credentials or fragment".into(),
        );
    }
    Ok((value.to_owned(), url.origin().ascii_serialization()))
}

/// Independently check the payment terms against the existing native vault request.
/// Unknown decoded charge fields fail closed; original challenge extensions are retained and bound.
pub fn validate_mpp_request(
    mpp: &MppPaymentRequest,
    chain_id: u64,
    token: &str,
    recipient: &str,
    amount: &str,
    now: u64,
) -> Result<MppPurchase> {
    if mpp.version != 1 || mpp.method != "GET" || chain_id != 42431 {
        return Err("MPP supports version 1 HTTPS GET Tempo testnet charge/push only".into());
    }
    let (url, origin) = mpp_url(&mpp.url)?;
    let challenge = mpp.challenge.as_object().ok_or("Invalid MPP challenge")?;
    if challenge.values().any(|v| !v.is_string()) {
        return Err("MPP challenge auth-params must be strings".into());
    }
    if challenge
        .get("header")
        .and_then(Value::as_str)
        .is_some_and(|header| {
            !header.eq_ignore_ascii_case("Authorization")
                && !header.eq_ignore_ascii_case("Payment-Authorization")
        })
    {
        return Err("Unsupported MPP credential header".into());
    }
    if serde_json::to_vec(challenge)
        .map_err(|_| "Invalid MPP challenge")?
        .len()
        > 16_384
    {
        return Err("MPP challenge is too large".into());
    }
    text(&mpp.challenge, "id", 512)?;
    text(&mpp.challenge, "realm", 512)?;
    if text(&mpp.challenge, "method", 32)? != "tempo"
        || text(&mpp.challenge, "intent", 32)? != "charge"
    {
        return Err("Expected an MPP Tempo charge challenge".into());
    }
    let encoded = text(&mpp.challenge, "request", 12_000)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "Invalid MPP encoded request")?;
    if URL_SAFE_NO_PAD.encode(&decoded) != encoded {
        return Err("Invalid MPP encoded request".into());
    }
    let request: Value = serde_json::from_slice(&decoded).map_err(|_| "Invalid MPP charge JSON")?;
    // The charge request is JCS in this narrow integer/string schema. Reject duplicate
    // keys and alternative spellings instead of authorizing an ambiguous interpretation.
    if crate::canonical_json(&request)? != decoded {
        return Err("MPP request must use canonical JSON".into());
    }
    let fields = request.as_object().ok_or("Invalid MPP charge JSON")?;
    if fields.keys().any(|k| {
        !matches!(
            k.as_str(),
            "amount" | "currency" | "recipient" | "description" | "externalId" | "methodDetails"
        )
    }) {
        return Err("Unsupported MPP charge field".into());
    }
    for field in ["description", "externalId"] {
        if fields.contains_key(field) {
            text(&request, field, 1024)?;
        }
    }
    let amount_units = text(&request, "amount", 20)?;
    if amount_units.starts_with('0') || !amount_units.bytes().all(|b| b.is_ascii_digit()) {
        return Err("MPP requires a positive exact base-unit amount".into());
    }
    let parsed = amount_units
        .parse::<u64>()
        .map_err(|_| "MPP amount exceeds u64")?;
    let token = address(token)?;
    let recipient = address(recipient)?;
    if parsed != units(amount)?
        || token == ZERO
        || recipient == ZERO
        || address(text(&request, "currency", 42)?)? != token
        || address(text(&request, "recipient", 42)?)? != recipient
    {
        return Err("MPP challenge changed the exact token, recipient or amount".into());
    }
    let details = request
        .get("methodDetails")
        .map(Value::as_object)
        .transpose_option()?;
    if details
        .and_then(|d| d.get("chainId"))
        .and_then(Value::as_u64)
        != Some(chain_id)
    {
        return Err("MPP requires an explicit Tempo testnet chain ID".into());
    }
    if let Some(d) = details {
        if d.keys().any(|k| {
            !matches!(
                k.as_str(),
                "chainId" | "feePayer" | "memo" | "supportedModes"
            )
        }) {
            return Err("Unsupported MPP method details or split payment".into());
        }
        if d.get("chainId")
            .is_some_and(|v| v.as_u64() != Some(chain_id))
            || d.get("feePayer")
                .is_some_and(|v| v.as_bool() != Some(false))
        {
            return Err("MPP push requires the configured chain and client-paid fees".into());
        }
        if let Some(modes) = d.get("supportedModes") {
            let modes = modes.as_array().ok_or("Invalid MPP modes")?;
            if modes.is_empty()
                || modes.len() > 2
                || modes
                    .iter()
                    .any(|v| !matches!(v.as_str(), Some("pull" | "push")))
                || !modes.iter().any(|v| v == "push")
                || modes.len() == 2 && modes[0] == modes[1]
            {
                return Err("MPP challenge does not support push mode".into());
            }
        }
    }
    // A merchant-specified memo is exact; never replace it with a local request ID.
    let memo = mpp_memo(&mpp.challenge)?;
    if let Some(value) = details.and_then(|d| d.get("memo")) {
        if unhex(value.as_str().ok_or("Invalid MPP memo")?)? != unhex(&memo)? {
            return Err("MPP memo must bind the original challenge realm and ID".into());
        }
    }
    let bytes = unhex(&memo)?;
    if bytes.len() != 32 || bytes.iter().all(|b| *b == 0) {
        return Err("MPP requires a nonzero bytes32 memo".into());
    }
    let expiry = if let Some(expires) = challenge.get("expires") {
        let expires = expires.as_str().ok_or("Invalid MPP expiry")?;
        let timestamp =
            chrono::DateTime::parse_from_rfc3339(expires).map_err(|_| "Invalid MPP expiry")?;
        let expiry = u64::try_from(timestamp.timestamp()).map_err(|_| "Invalid MPP expiry")?;
        if expiry <= now {
            return Err("MPP challenge has expired".into());
        }
        Some(expiry)
    } else {
        None
    };
    Ok(MppPurchase {
        protocol: "mpp".into(),
        method: mpp.method.clone(),
        url,
        origin,
        request_digest: sha256(&json!(mpp))?,
        challenge_digest: sha256(&mpp.challenge)?,
        chain_id,
        token,
        recipient,
        amount_units: parsed.to_string(),
        memo: hex(&bytes),
        challenge_expires_at: expiry,
    })
}

trait TransposeOption<T> {
    fn transpose_option(self) -> Result<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Result<Option<T>> {
        match self {
            None => Ok(None),
            Some(Some(v)) => Ok(Some(v)),
            Some(None) => Err("Invalid MPP method details".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(terms: Value) -> MppPaymentRequest {
        MppPaymentRequest {
            version: 1,
            method: "GET".into(),
            url: "https://merchant.example/paid".into(),
            challenge: json!({"id":"charge-1","realm":"merchant.example","method":"tempo","intent":"charge","request":URL_SAFE_NO_PAD.encode(crate::canonical_json(&terms).unwrap()),"expires":"1970-01-01T00:05:00Z","opaque":"original-opaque"}),
        }
    }
    fn terms() -> Value {
        json!({"amount":"250000","currency":hex(&[13;20]),"recipient":hex(&[19;20]),"methodDetails":{"chainId":42431,"feePayer":false,"supportedModes":["push"]}})
    }
    fn check(mpp: &MppPaymentRequest) -> Result<MppPurchase> {
        validate_mpp_request(mpp, 42431, &hex(&[13; 20]), &hex(&[19; 20]), "0.25", 100)
    }
    #[test]
    fn memo_and_digests_bind_original_challenge_and_http_request() {
        let mpp = request(terms());
        let verified = check(&mpp).unwrap();
        assert_eq!(verified.challenge_expires_at, Some(300));
        assert_eq!(verified.memo, mpp_memo(&mpp.challenge).unwrap());
        assert_eq!(
            verified.memo,
            "0xef1ed71201e6dc21f2ed5ac85546af000000000000000000000eeb3775af5937"
        );
        let mut changed = mpp.clone();
        changed.challenge["opaque"] = json!("different-opaque");
        let other = check(&changed).unwrap();
        assert_ne!(verified.challenge_digest, other.challenge_digest);
        assert_ne!(verified.request_digest, other.request_digest);
        assert_eq!(verified.memo, other.memo);
        changed = mpp.clone();
        changed.url = "https://merchant.example/another".into();
        assert_ne!(
            verified.request_digest,
            check(&changed).unwrap().request_digest
        );
        changed = mpp.clone();
        changed.challenge["id"] = json!("charge-2");
        assert_ne!(verified.memo, check(&changed).unwrap().memo);
        changed = mpp;
        changed.challenge["realm"] = json!("other.example");
        assert_ne!(verified.memo, check(&changed).unwrap().memo);
    }
    #[test]
    fn unsupported_charge_terms_fail_before_any_payment() {
        for (path, value) in [
            ("amount", json!("0")),
            ("amount", json!("0250000")),
            ("amount", json!("0.25")),
            ("amount", json!("250001")),
            ("currency", json!(hex(&[22; 20]))),
            ("recipient", json!(hex(&[22; 20]))),
            ("extra", json!(true)),
        ] {
            let mut t = terms();
            t[path] = value;
            assert!(check(&request(t)).is_err(), "{path}");
        }
        for (path, value) in [
            ("feePayer", json!(true)),
            ("feePayer", json!("false")),
            ("chainId", json!(4217)),
            ("chainId", json!("42431")),
            ("supportedModes", json!(["pull"])),
            ("supportedModes", json!(["push", "push"])),
            ("supportedModes", json!(["unknown", "push"])),
            ("splits", json!([])),
            ("memo", json!(hex(&[0; 32]))),
            ("memo", json!(null)),
        ] {
            let mut t = terms();
            t["methodDetails"][path] = value;
            assert!(check(&request(t)).is_err(), "{path}");
        }
        let mut defaults = terms();
        defaults.as_object_mut().unwrap().remove("methodDetails");
        assert!(check(&request(defaults)).is_err());
        let mut missing_chain = terms();
        missing_chain["methodDetails"]
            .as_object_mut()
            .unwrap()
            .remove("chainId");
        assert!(check(&request(missing_chain)).is_err());
    }
    #[test]
    fn malformed_urls_expiry_and_envelopes_fail_closed() {
        for url in [
            "http://merchant.example/paid",
            "https://merchant.example/paid?q=1",
            "https://merchant.example/paid#x",
            "https://user:password@merchant.example/paid",
            "https://merchant.example:443/paid",
            "/paid",
        ] {
            let mut mpp = request(terms());
            mpp.url = url.into();
            assert!(check(&mpp).is_err());
        }
        let mut mpp = request(terms());
        mpp.method = "POST".into();
        assert!(check(&mpp).is_err());
        mpp = request(terms());
        mpp.challenge["expires"] = json!("1970-01-01T00:01:40Z");
        assert!(check(&mpp).is_err());
        mpp = request(terms());
        mpp.challenge["expires"] = json!(null);
        assert!(check(&mpp).is_err());
        mpp = request(terms());
        mpp.challenge["request"] =
            json!(format!("{}=", mpp.challenge["request"].as_str().unwrap()));
        assert!(check(&mpp).is_err());
        mpp = request(terms());
        mpp.challenge["intent"] = json!("session");
        assert!(check(&mpp).is_err());
        assert!(validate_mpp_request(
            &request(terms()),
            31337,
            &hex(&[13; 20]),
            &hex(&[19; 20]),
            "0.25",
            100
        )
        .is_err());
        for header in ["Authorization", "Payment-Authorization", "authorization"] {
            let mut mpp = request(terms());
            mpp.challenge["header"] = json!(header);
            assert!(check(&mpp).is_ok());
        }
        for header in ["Cookie", "X-API-Key", "", "Authorization\r\nCookie"] {
            let mut mpp = request(terms());
            mpp.challenge["header"] = json!(header);
            assert!(check(&mpp).is_err());
        }
    }
    #[test]
    fn explicit_merchant_memo_is_preserved_exactly() {
        let expected = mpp_memo(&request(terms()).challenge).unwrap();
        let mut t = terms();
        t["methodDetails"]["memo"] = json!(expected);
        assert_eq!(check(&request(t)).unwrap().memo, expected);
        let mut arbitrary = terms();
        arbitrary["methodDetails"]["memo"] = json!(hex(&[77; 32]));
        assert!(check(&request(arbitrary)).is_err());
    }
}
