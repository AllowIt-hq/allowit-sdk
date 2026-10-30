//! Native Stellar owner signing through a public browser fragment.
//! Backs `allowit wallet configure`: checks the owner account state and prints a
//! link whose fragment holds the public setup request for the owner to sign.
use crate::{
    error::{Error, Result},
    routing,
};
use allowit_client_core::store::Store;
use allowit_stellar_wallet::{
    AccountState,
    owner::{NativeSetup, prepare_native_setup, validate_native_setup},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn signing_url(origin: &str, setup: &NativeSetup) -> Result<String> {
    validate_native_setup(setup).map_err(Error::config)?;
    let origin = crate::config::parse_origin(origin)?;
    let bytes = serde_json::to_vec(setup)
        .map_err(|_| Error::config("Cannot encode Stellar owner intent"))?;
    Ok(format!(
        "{origin}/?chain=stellar&network=stellar%3Atestnet#allowit-stellar-owner={}",
        URL_SAFE_NO_PAD.encode(bytes)
    ))
}

fn fetch(path: &str) -> Result<serde_json::Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| Error::config("Cannot initialize Stellar transport"))?;
    let response = client
        .get(format!("https://horizon-testnet.stellar.org{path}"))
        .send()
        .map_err(|_| Error::config("Stellar Testnet state is unavailable"))?;
    if !response.status().is_success() {
        return Err(Error::config("Stellar Testnet rejected the state request"));
    }
    let mut bytes = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::config("Cannot read Stellar state"))?;
    if bytes.len() > 1_048_576 {
        return Err(Error::config("Stellar state exceeds bounds"));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::config("Invalid Stellar state"))
}
fn clock() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| Error::config("System time is unavailable"))
}

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    let usage = "allowit wallet configure --network stellar:testnet --owner G_ADDRESS --agent G_ADDRESS --engine G_ADDRESS --request-id ID [--fee-per-operation 100]";
    let mut flags = BTreeMap::new();
    if !args.len().is_multiple_of(2) {
        return Err(Error::usage(usage));
    }
    for pair in args.as_chunks::<2>().0 {
        if ![
            "--owner",
            "--agent",
            "--engine",
            "--request-id",
            "--fee-per-operation",
        ]
        .contains(&pair[0].as_str())
            || flags.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err(Error::usage(usage));
        }
    }
    let network = routing::selected().ok_or_else(|| Error::usage(usage))?;
    if network != "stellar:testnet" {
        return Err(Error::unsupported(
            "Native browser setup supports Stellar Testnet only",
        ));
    }
    let required = |name| flags.get(name).copied().ok_or_else(|| Error::usage(usage));
    let owner = required("--owner")?;
    let agent = required("--agent")?;
    let engine = required("--engine")?;
    let id = required("--request-id")?;
    for address in [owner, agent, engine] {
        allowit_stellar_wallet::account_key(address).map_err(Error::usage)?;
    }
    if !crate::request::request_id_valid(id) {
        return Err(Error::usage("Invalid Stellar setup request ID"));
    }
    let fee = flags
        .get("--fee-per-operation")
        .unwrap_or(&"100")
        .parse::<u32>()
        .map_err(|_| Error::usage("Invalid setup fee"))?;
    if !(100..=100_000).contains(&fee) {
        return Err(Error::usage(
            "Setup fee must be 100..100000 stroops per operation",
        ));
    }
    let store = Store::open(Path::new(".allowit/stellar-owner-journal"))?;
    let setup = if let Some(saved) = store.load::<NativeSetup>(id)? {
        if saved.owner != owner
            || saved.agent != agent
            || saved.engine != engine
            || saved.network != network
            || saved.fee_stroops != fee * 3
        {
            return Err(Error::config(
                "This Stellar setup request ID already binds another intent",
            ));
        }
        validate_native_setup(&saved).map_err(Error::config)?;
        saved
    } else {
        let root = fetch("/")?;
        if root["network_passphrase"] != "Test SDF Network ; September 2015" {
            return Err(Error::config("Stellar endpoint network differs"));
        }
        let state = fetch(&format!("/accounts/{owner}"))?;
        if state["account_id"] != owner
            || state["subentry_count"] != 0
            || state["signers"].as_array().is_none_or(|s| s.len() != 1)
            || state["signers"][0]["key"] != owner
            || state["signers"][0]["type"] != "ed25519_public_key"
            || state["signers"][0]["weight"] != 1
        {
            return Err(Error::config(
                "Setup requires a fresh funded owner account without signers, offers or trustlines",
            ));
        }
        let threshold = |key| {
            state["thresholds"][key]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| Error::config("Invalid current thresholds"))
        };
        let current = AccountState {
            address: owner.into(),
            sequence: state["sequence"]
                .as_str()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| Error::config("Invalid account sequence"))?,
            master_weight: 1,
            low_threshold: threshold("low_threshold")?,
            medium_threshold: threshold("med_threshold")?,
            high_threshold: threshold("high_threshold")?,
            signers: vec![],
        };
        let now = clock()?;
        let prepared = prepare_native_setup(&current, agent, engine, id, &network, fee, now)
            .map_err(Error::config)?;
        store.save(id, &prepared)?;
        prepared
    };
    let url = signing_url(&crate::config::env_value("ALLOWIT_URL"), &setup)?;
    let expired = setup.expires_at <= clock()?;
    *stdout += &crate::output::json(
        &serde_json::json!({"state":if expired {"owner_intent_expired"}else{"awaiting_owner_signature"},"executed":false,"recoveryOnly":expired,"requestId":id,"signingUrl":url,"intentDigest":setup.intent_digest,"transactionHash":setup.transaction_hash}),
    );
    Ok(10)
}
