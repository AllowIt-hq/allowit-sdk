//! Public continuation for an existing owner-configured Native Stellar policy.
use crate::{
    error::{Error, Result},
    routing,
};
use allowit_client_core::store::Store;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyBinding {
    pub wallet: String,
    pub route: String,
    pub policy_instance: String,
    pub module_digest: String,
    pub asset: String,
    pub asset_symbol: String,
    pub asset_issuer: Option<String>,
    pub max_fee_stroops: u32,
    pub deployment_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalIntent {
    pub version: u32,
    pub profile: String,
    pub request_id: String,
    pub network: String,
    pub owner: String,
    pub policy_id: String,
    pub revision: u32,
    pub source: String,
    pub source_hash: String,
    pub ir_hash: String,
    pub binding: PolicyBinding,
    pub intent_digest: String,
}
fn hash(v: impl AsRef<[u8]>) -> String {
    Sha256::digest(v.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn intent_digest(v: &ApprovalIntent) -> Result<String> {
    let binding = serde_json_canonicalizer::to_vec(&v.binding)
        .map_err(|_| Error::config("Invalid Stellar policy binding"))?;
    Ok(hash(
        serde_json::to_vec(&serde_json::json!([
            "allowit-stellar-approval-v1",
            v.request_id,
            v.network,
            v.owner,
            v.policy_id,
            v.revision,
            v.source_hash,
            v.ir_hash,
            hash(binding)
        ]))
        .map_err(|_| Error::config("Cannot encode Stellar approval identity"))?,
    ))
}
pub fn validate(v: &ApprovalIntent) -> Result<()> {
    let b = &v.binding;
    if v.version != 1
        || v.profile != "allowit-stellar-approval-v1"
        || v.network != "stellar:testnet"
        || !crate::request::request_id_valid(&v.request_id)
        || v.policy_id.is_empty()
        || v.policy_id.len() > 128
        || v.policy_id.chars().any(char::is_control)
        || v.revision == 0
        || v.source.len() > 32_768
        || v.source_hash != hash(v.source.as_bytes())
        || v.ir_hash.len() != 64
        || !v
            .ir_hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || b.route != "native"
        || b.asset_symbol.is_empty()
        || b.asset_symbol.len() > 12
        || !b.asset_symbol.bytes().all(|b| b.is_ascii_alphanumeric())
        || b.max_fee_stroops < 100
        || b.max_fee_stroops > 1_000_000
    {
        return Err(Error::config("Invalid Native Stellar policy approval"));
    }
    for a in [&v.owner, &b.wallet] {
        allowit_stellar_wallet::account_key(a).map_err(Error::config)?;
    }
    if let Some(a) = &b.asset_issuer {
        allowit_stellar_wallet::account_key(a).map_err(Error::config)?;
    }
    if !b.asset.starts_with('C') {
        return Err(Error::config(
            "The installed Stellar asset must be its SAC address",
        ));
    }
    allowit_stellar_wallet::address(&b.asset).map_err(Error::config)?;
    for h in [&b.policy_instance, &b.module_digest] {
        allowit_stellar_wallet::unhex::<32>(h).map_err(Error::config)?;
        if !h
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::config(
                "Stellar policy identities must use lowercase hexadecimal",
            ));
        }
    }
    if let Some(h) = &b.deployment_id {
        allowit_stellar_wallet::unhex::<32>(h).map_err(Error::config)?;
        if !h
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::config(
                "Stellar deployment identity must use lowercase hexadecimal",
            ));
        }
    }
    if intent_digest(v)? != v.intent_digest {
        return Err(Error::config("The Stellar policy approval changed"));
    }
    Ok(())
}
pub fn signing_url(origin: &str, v: &ApprovalIntent) -> Result<String> {
    validate(v)?;
    let origin = crate::config::parse_origin(origin)?;
    let bytes =
        serde_json::to_vec(v).map_err(|_| Error::config("Cannot encode Stellar approval"))?;
    let encoded = URL_SAFE_NO_PAD.encode(bytes);
    if encoded.len() > 65_000 {
        return Err(Error::config("Stellar approval exceeds browser bounds"));
    }
    Ok(format!(
        "{origin}/?chain=stellar&network=stellar%3Atestnet#allowit-stellar-approval={encoded}"
    ))
}
/// Select only public approval terms from a stored policy export. Secret or unrelated fields are not copied.
pub fn approval_for_policy(
    policy: &serde_json::Value,
    owner: &str,
    id: &str,
) -> Result<ApprovalIntent> {
    let text = |k| {
        policy[k]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::config("The public Stellar policy is missing source identity"))
    };
    if policy["network"] != "stellar:testnet"
        || policy["runtime"]["wallet"]["profile"] != "allowit-stellar-native-v1"
    {
        return Err(Error::config(
            "Use an existing installed Native Stellar Testnet policy",
        ));
    }
    let raw = &policy["runtime"]["instanceBinding"];
    let binding = PolicyBinding {
        wallet: raw["wallet"].as_str().unwrap_or("").into(),
        route: raw["route"].as_str().unwrap_or("").into(),
        policy_instance: raw["policyInstance"].as_str().unwrap_or("").into(),
        module_digest: raw["moduleDigest"].as_str().unwrap_or("").into(),
        asset: text("mint")?,
        asset_symbol: text("token")?,
        asset_issuer: raw["assetIssuer"].as_str().map(str::to_owned),
        max_fee_stroops: raw["maxFeeStroops"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        deployment_id: raw["deploymentId"].as_str().map(str::to_owned),
    };
    if raw["asset"] != binding.asset || raw["assetSymbol"] != binding.asset_symbol {
        return Err(Error::config(
            "The policy asset differs from its installation",
        ));
    }
    let mut intent = ApprovalIntent {
        version: 1,
        profile: "allowit-stellar-approval-v1".into(),
        request_id: id.into(),
        network: "stellar:testnet".into(),
        owner: owner.into(),
        policy_id: text("id")?,
        revision: policy["revision"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        source: text("code")?,
        source_hash: text("sourceHash")?,
        ir_hash: text("irHash")?,
        binding,
        intent_digest: String::new(),
    };
    intent.intent_digest = intent_digest(&intent)?;
    validate(&intent)?;
    Ok(intent)
}
pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    let usage = "allowit wallet approve --network stellar:testnet --policy-file PUBLIC_POLICY_JSON --owner G_ADDRESS --request-id ID";
    let mut flags = std::collections::BTreeMap::new();
    if !args.len().is_multiple_of(2) {
        return Err(Error::usage(usage));
    }
    for p in args.as_chunks::<2>().0 {
        if !["--policy-file", "--owner", "--request-id"].contains(&p[0].as_str())
            || flags.insert(p[0].as_str(), p[1].as_str()).is_some()
        {
            return Err(Error::usage(usage));
        }
    }
    let get = |k| flags.get(k).copied().ok_or_else(|| Error::usage(usage));
    let owner = get("--owner")?;
    let id = get("--request-id")?;
    if routing::selected().as_deref() != Some("stellar:testnet") {
        return Err(Error::unsupported(
            "Native Stellar approval links support Testnet only",
        ));
    }
    let file = std::fs::File::open(get("--policy-file")?)
        .map_err(|_| Error::config("Cannot read public Stellar policy file"))?;
    let mut bytes = Vec::new();
    file.take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::config("Cannot read public Stellar policy file"))?;
    if bytes.len() > 131_072 {
        return Err(Error::config("Public Stellar policy file exceeds bounds"));
    }
    let policy: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::config("Invalid public Stellar policy JSON"))?;
    let intent = approval_for_policy(&policy, owner, id)?;
    let store = Store::open(Path::new(".allowit/stellar-approval-journal"))?;
    if store
        .load::<ApprovalIntent>(id)?
        .as_ref()
        .is_some_and(|v| v != &intent)
    {
        return Err(Error::config(
            "This approval request ID already binds another policy",
        ));
    }
    store.save(id, &intent)?;
    let url = signing_url(&crate::config::env_value("ALLOWIT_URL"), &intent)?;
    *stdout += &crate::output::json(
        &serde_json::json!({"state":"awaiting_owner_approval","executed":false,"movesFunds":false,"requestId":id,"signingUrl":url,"intentDigest":intent.intent_digest,"policyId":intent.policy_id}),
    );
    Ok(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!("../tests/fixtures/stellar-approval.json")).unwrap()
    }
    #[test]
    fn public_approval_binds_original_source_and_filters_secret_fields() {
        let f = fixture();
        let expected: ApprovalIntent = serde_json::from_value(f["intent"].clone()).unwrap();
        let mut policy = f["policy"].clone();
        policy["signingSeed"] = serde_json::json!("never-export-this-secret");
        let intent = approval_for_policy(&policy, &expected.owner, &expected.request_id).unwrap();
        assert_eq!(intent, expected);
        let url = signing_url("https://app.allowit.xyz", &intent).unwrap();
        assert!(url.contains("?chain=stellar&network=stellar%3Atestnet#allowit-stellar-approval="));
        assert!(
            !serde_json::to_string(&intent)
                .unwrap()
                .contains("never-export")
        );
        policy["code"] = serde_json::json!("substituted source");
        assert!(approval_for_policy(&policy, &expected.owner, &expected.request_id).is_err());
    }
    #[test]
    fn altered_binding_profile_and_network_are_refused() {
        let f = fixture();
        let mut intent: ApprovalIntent = serde_json::from_value(f["intent"].clone()).unwrap();
        intent.binding.module_digest = "11".repeat(32);
        assert!(validate(&intent).is_err());
        let mut policy = f["policy"].clone();
        policy["network"] = serde_json::json!("stellar:mainnet");
        assert!(approval_for_policy(&policy, &intent.owner, &intent.request_id).is_err());
        policy = f["policy"].clone();
        policy["runtime"]["wallet"]["profile"] = serde_json::json!("allowit-stellar-odc-v1");
        assert!(approval_for_policy(&policy, &intent.owner, &intent.request_id).is_err());
    }
    #[test]
    fn native_approval_preserves_separate_owner_authority_and_canonical_asset_identity() {
        let f = fixture();
        let mut intent: ApprovalIntent = serde_json::from_value(f["intent"].clone()).unwrap();
        // Existing installations can give a distinct owner weight-three administration authority.
        intent.owner = allowit_stellar_wallet::account_for_seed(&[9; 32]);
        intent.intent_digest = intent_digest(&intent).unwrap();
        validate(&intent).unwrap();
        for symbol in ["XLM\n", "USDC\u{202e}", "TOO_LONG_ASSET"] {
            let mut altered = intent.clone();
            altered.binding.asset_symbol = symbol.into();
            altered.intent_digest = intent_digest(&altered).unwrap();
            assert!(validate(&altered).is_err());
        }
        intent.ir_hash = "AB".repeat(32);
        intent.intent_digest = intent_digest(&intent).unwrap();
        assert!(validate(&intent).is_err());
    }
}
