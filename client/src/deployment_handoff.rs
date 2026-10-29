//! Exact, unsigned owner deployment intent for review in the browser.
use crate::{
    config::parse_origin,
    error::{Error, Result},
};
use allowit_native::{
    client::Deployment,
    crypto::Key,
    policy::{Policy, decimal, digest, units},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const DEVNET: &str = "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1";
const PROFILE: &str = "allowit-deploy-v1";
const FRAGMENT: &str = "#allowit-deploy=";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeploymentHandoff {
    pub version: u8,
    pub profile: String,
    pub request_id: String,
    pub network: String,
    pub owner: String,
    pub amount: String,
    pub policy: Policy,
    pub deployment: Deployment,
    pub intent_digest: String,
}
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: std::collections::BTreeMap<_, _> =
                map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        value => value.clone(),
    }
}
impl DeploymentHandoff {
    /// Build a review request. This function never loads a signing key or submits a transaction.
    pub fn new(
        policy: Policy,
        deployment: Deployment,
        owner: Key,
        amount: &str,
        request_id: Option<&str>,
    ) -> Result<Self> {
        policy
            .validate()
            .map_err(|e| Error::config(e.to_string()))?;
        if policy.network != "solana:devnet" || deployment.network != policy.network {
            return Err(Error::unsupported(
                "Browser deployment supports Solana Devnet only; no transaction was prepared or sent",
            ));
        }
        if deployment.source_bundle != policy.source_bundle || !owner.on_curve() {
            return Err(Error::config(
                "Deployment release or owner does not match the policy",
            ));
        }
        let amount = units(amount).map_err(|e| Error::usage(e.to_string()))?;
        if amount == 0 {
            return Err(Error::usage("Deployment amount must be positive"));
        }
        let mut handoff = Self {
            version: 1,
            profile: PROFILE.into(),
            request_id: String::new(),
            network: DEVNET.into(),
            owner: owner.to_string(),
            amount: decimal(amount),
            policy,
            deployment,
            intent_digest: String::new(),
        };
        handoff.request_id = match request_id {
            Some(id) => {
                if !crate::request::request_id_valid(id) {
                    return Err(Error::usage(
                        "ALLOWIT_REQUEST_ID must be 8 to 100 characters from A-Z a-z 0-9 . _ : -",
                    ));
                }
                id.into()
            }
            None => format!("deploy-{}", &digest(handoff.digest_input()?)[..40]),
        };
        handoff.intent_digest = digest(handoff.digest_input()?);
        Ok(handoff)
    }
    fn digest_input(&self) -> Result<Vec<u8>> {
        let deployment = serde_json::to_value(&self.deployment)
            .map_err(|_| Error::config("Invalid deployment manifest"))?;
        let deployment = serde_json::to_vec(&canonical(&deployment))
            .map_err(|_| Error::config("Invalid deployment manifest"))?;
        serde_json::to_vec(&json!([
            PROFILE,
            self.request_id,
            self.network,
            self.owner,
            self.amount,
            self.policy.id,
            digest(self.policy.rust.as_bytes()),
            digest(deployment)
        ]))
        .map_err(|_| Error::config("Invalid deployment intent"))
    }
    pub fn signing_url(&self, origin: &str) -> Result<String> {
        if self.version != 1
            || self.profile != PROFILE
            || self.intent_digest != digest(self.digest_input()?)
        {
            return Err(Error::config("Saved browser deployment intent changed"));
        }
        let payload =
            serde_json::to_vec(self).map_err(|_| Error::config("Invalid deployment handoff"))?;
        let fragment = format!("{FRAGMENT}{}", URL_SAFE_NO_PAD.encode(payload));
        if fragment.len() > 65_536 {
            return Err(Error::usage("Browser deployment handoff exceeds 64 KiB"));
        }
        Ok(format!("{}/?chain=solana{fragment}", parse_origin(origin)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> DeploymentHandoff {
        let policy =
            Policy::generate("solana:devnet", "Spend up to 5 test tokens per day").unwrap();
        let key = Key::parse("11111111111111111111111111111111").unwrap();
        let owner = Key(ed25519_dalek::SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .to_bytes());
        let deployment = Deployment {
            network: policy.network.clone(),
            source_bundle: policy.source_bundle.clone(),
            policy: key,
            policy_data: key,
            custody: key,
        };
        DeploymentHandoff::new(
            policy,
            deployment,
            owner,
            "5.000000",
            Some("deployment-0001"),
        )
        .unwrap()
    }
    #[test]
    fn browser_link_binds_exact_unsigned_intent() {
        let handoff = fixture();
        let url = handoff.signing_url("https://app.allowit.xyz").unwrap();
        let encoded = url.split(FRAGMENT).nth(1).unwrap();
        let decoded: DeploymentHandoff =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap();
        assert_eq!(decoded.intent_digest, handoff.intent_digest);
        assert_eq!(decoded.amount, "5");
        let mut changed = decoded;
        changed.amount = "6".into();
        assert!(changed.signing_url("https://app.allowit.xyz").is_err());
    }
    #[test]
    fn cross_language_fixture_matches_rust_digest() {
        let handoff: DeploymentHandoff =
            serde_json::from_str(include_str!("../tests/fixtures/deployment-handoff.json"))
                .unwrap();
        handoff.policy.validate().unwrap();
        assert!(handoff.signing_url("https://app.allowit.xyz").is_ok());
    }
}
