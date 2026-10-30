//! Browser owner-signing intent. Contains public metadata only.
//! `allowit policy deploy AMOUNT` records the intent and prints the browser link
//! that the owner opens to sign the vault deployment.
use crate::{
    error::{Error, Result},
    routing,
};
use allowit_native::{
    client::{Config, Deployment},
    crypto::Key,
    policy::Policy,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use sha2::{Digest, Sha256};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Wallet {
    mint: Key,
    executor: Key,
    authority: Key,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Handoff<'a> {
    version: u32,
    profile: &'static str,
    request_id: &'a str,
    network: &'a str,
    owner: Key,
    amount: &'a str,
    policy: &'a Policy,
    deployment: &'a Deployment,
    wallet: Wallet,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_preparation_of: Option<&'a str>,
    intent_digest: String,
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn signing_url(
    origin: &str,
    request_id: &str,
    owner: Key,
    amount: &str,
    policy: &Policy,
    config: &Config,
) -> Result<String> {
    signing_url_with_retry(origin, request_id, owner, amount, policy, config, None)
}
/// Explicit replacement of an authoritatively expired unsigned preparation.
pub fn signing_url_with_retry(
    origin: &str,
    request_id: &str,
    owner: Key,
    amount: &str,
    policy: &Policy,
    config: &Config,
    retry_preparation_of: Option<&str>,
) -> Result<String> {
    if let Some(prior) = retry_preparation_of
        && (prior == request_id || !crate::request::request_id_valid(prior))
    {
        return Err(Error::usage(
            "Retry requires a distinct valid prior preparation ID",
        ));
    }
    if policy.network != "solana:devnet" {
        return Err(Error::unsupported(
            "Browser policy deployment supports the pinned Solana Devnet release only",
        ));
    }
    policy
        .validate()
        .map_err(|_| Error::config("Invalid policy for browser deployment"))?;
    if !crate::request::request_id_valid(request_id) {
        return Err(Error::usage("Invalid deployment request ID"));
    }
    let deployment = config
        .deployment
        .as_ref()
        .ok_or_else(|| Error::config("Missing public deployment"))?;
    if config.network != policy.network
        || deployment.network != policy.network
        || deployment.source_bundle != policy.source_bundle
    {
        return Err(Error::config(
            "Deployment differs from policy network or source binding",
        ));
    }
    let wallet = Wallet {
        mint: config
            .mint
            .ok_or_else(|| Error::config("Missing public mint"))?,
        executor: config
            .executor
            .ok_or_else(|| Error::config("Missing public agent executor"))?,
        authority: config
            .authority
            .ok_or_else(|| Error::config("Missing public authority"))?,
    };
    let units = allowit_native::policy::units(amount)
        .map_err(|_| Error::usage("Invalid deployment amount"))?;
    if units == 0 {
        return Err(Error::usage("Deployment funding must be positive"));
    }
    let amount = allowit_native::policy::decimal(units);
    let network = routing::caip2(&policy.network)?;
    let binding = serde_json_canonicalizer::to_vec(deployment)
        .map_err(|_| Error::config("Cannot encode deployment"))?;
    let public_wallet = serde_json_canonicalizer::to_vec(&wallet)
        .map_err(|_| Error::config("Cannot encode public wallet"))?;
    let mut terms = serde_json::json!([
        "allowit-deploy-v1",
        request_id,
        network,
        owner.to_string(),
        amount,
        policy.id,
        hash(policy.rust.as_bytes()),
        hash(&binding),
        hash(&public_wallet)
    ]);
    if let Some(prior) = retry_preparation_of {
        terms.as_array_mut().unwrap().push(serde_json::json!(prior));
    }
    let digest = hash(&serde_json::to_vec(&terms).unwrap());
    let handoff = Handoff {
        version: 1,
        profile: "allowit-deploy-v1",
        request_id,
        network,
        owner,
        amount: &amount,
        policy,
        deployment,
        wallet,
        retry_preparation_of,
        intent_digest: digest,
    };
    let bytes =
        serde_json::to_vec(&handoff).map_err(|_| Error::config("Cannot encode browser handoff"))?;
    if bytes.len() > 48 * 1024 {
        return Err(Error::usage("Deployment handoff exceeds browser bounds"));
    }
    let origin = crate::config::parse_origin(origin)?;
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    if encoded.len() + "#allowit-deploy=".len() > 65536 {
        return Err(Error::usage(
            "Deployment handoff exceeds browser fragment bounds",
        ));
    }
    Ok(format!("{origin}/?chain=solana#allowit-deploy={encoded}"))
}

/// Reads the public review fingerprint from a locally constructed handoff URL.
pub fn intent_digest_from_url(url: &str) -> Result<String> {
    let encoded = url
        .split("#allowit-deploy=")
        .nth(1)
        .ok_or_else(|| Error::config("Invalid browser signing URL"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| Error::config("Invalid signing URL encoding"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| Error::config("Invalid signing intent"))?;
    value["intentDigest"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| Error::config("Missing signing intent fingerprint"))
}
#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> (Policy, Config) {
        let policy =
            Policy::generate("solana:devnet", "Spend up to 5 test tokens per day").unwrap();
        let config = Config {
            network: policy.network.clone(),
            mint: Some(Key([4; 32])),
            executor: Some(Key([2; 32])),
            authority: Some(Key([3; 32])),
            deployment: Some(Deployment {
                network: policy.network.clone(),
                source_bundle: policy.source_bundle.clone(),
                policy: Key([7; 32]),
                policy_data: Key([8; 32]),
                custody: Key([9; 32]),
            }),
        };
        (policy, config)
    }
    fn payload(url: &str) -> serde_json::Value {
        let bytes = URL_SAFE_NO_PAD
            .decode(url.split("#allowit-deploy=").nth(1).unwrap())
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }
    #[test]
    fn exact_public_roles_amount_and_source_are_bound() {
        let (policy, mut config) = fixture();
        let a = signing_url(
            "https://app.allowit.xyz",
            "deployment-test-1",
            Key([1; 32]),
            "1.000000",
            &policy,
            &config,
        )
        .unwrap();
        let original = payload(&a);
        assert_eq!(original["amount"], "1");
        assert_eq!(
            original["network"],
            "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"
        );
        assert!(original.get("seed").is_none());
        config.executor = Some(Key([5; 32]));
        let changed = payload(
            &signing_url(
                "https://app.allowit.xyz",
                "deployment-test-1",
                Key([1; 32]),
                "1",
                &policy,
                &config,
            )
            .unwrap(),
        );
        assert_ne!(original["intentDigest"], changed["intentDigest"]);
        assert!(
            signing_url(
                "https://app.allowit.xyz",
                "bad",
                Key([1; 32]),
                "1",
                &policy,
                &config
            )
            .is_err()
        );
    }
    #[test]
    fn unsupported_network_or_changed_manifest_is_refused() {
        let (mut policy, mut config) = fixture();
        config.deployment.as_mut().unwrap().source_bundle = "changed".into();
        assert!(
            signing_url(
                "https://app.allowit.xyz",
                "deployment-test-1",
                Key([1; 32]),
                "1",
                &policy,
                &config
            )
            .is_err()
        );
        policy.network = "solana:mainnet".into();
        assert!(
            signing_url(
                "https://app.allowit.xyz",
                "deployment-test-1",
                Key([1; 32]),
                "1",
                &policy,
                &config
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    #[test]
    fn retry_identity_is_optional_bound_and_never_reuses_current_id() {
        let (p, c) = super::tests::fixture();
        let old = signing_url(
            "https://app.allowit.xyz",
            "deployment-test-1",
            Key([1; 32]),
            "1",
            &p,
            &c,
        )
        .unwrap();
        let new = signing_url_with_retry(
            "https://app.allowit.xyz",
            "deployment-test-1",
            Key([1; 32]),
            "1",
            &p,
            &c,
            Some("deployment-test-0"),
        )
        .unwrap();
        assert_ne!(
            intent_digest_from_url(&old).unwrap(),
            intent_digest_from_url(&new).unwrap()
        );
        assert!(
            signing_url_with_retry(
                "https://app.allowit.xyz",
                "deployment-test-1",
                Key([1; 32]),
                "1",
                &p,
                &c,
                Some("deployment-test-1")
            )
            .is_err()
        );
        assert!(
            signing_url_with_retry(
                "https://app.allowit.xyz",
                "deployment-test-1",
                Key([1; 32]),
                "1",
                &p,
                &c,
                Some("bad")
            )
            .is_err()
        );
    }
}
