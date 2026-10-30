//! `allowit setup`: registers public wallet metadata with the selected account.
//! Private keys, seeds and wallet files stay on this device.
//!
//! Input: `--input JSON` or `--input-file PATH` with `network` (canonical CAIP-2),
//! `address`, `publicKey`, `keyType` and optional `name`. Posts to
//! `/api/gateway/setup` with `ALLOWIT_ACCOUNT_TOKEN`; refuses `--policy`.
//!
//! ```text
//! allowit wallet generate --network stellar:testnet --output agent.key
//! allowit wallet show --network stellar:testnet agent.key > agent.json
//! allowit setup --input-file agent.json --request-id setup-0001
//! allowit whoami
//! ```
use crate::{
    error::{Error, Result},
    gateway::{args::Invocation, control},
};
use serde_json::Value;

const FIELDS: &[&str] = &["network", "address", "publicKey", "keyType", "name"];

pub(crate) fn run(inv: &Invocation, stdout: &mut String, stderr: &mut String) -> Result<i32> {
    let c = control::input(inv)?;
    if !c.pos.is_empty() {
        return Err(Error::usage(
            "use --input JSON or named fields for this account operation",
        ));
    }
    control::require_account_authority(inv)?;
    registration(&c.input)?;
    control::mutate(inv, "/api/gateway/setup", "setup", c.input, stdout, stderr)
}

/// Accepts only public wallet metadata that matches its own public key.
fn registration(input: &Value) -> Result<()> {
    control::public_fields(input, FIELDS)?;
    let mut registration = input.clone();
    registration.as_object_mut().unwrap().remove("name");
    let wallet: crate::wallet::PublicWallet =
        serde_json::from_value(registration).map_err(|_| {
            Error::usage(
                "Setup requires network, address, publicKey and keyType from the local wallet",
            )
        })?;
    wallet.validate()?;
    if crate::routing::caip2(&wallet.network)? != wallet.network {
        return Err(Error::usage(
            "Wallet registration requires its canonical x402 network identifier",
        ));
    }
    if crate::routing::selected().is_some_and(|network| {
        crate::routing::normalize(&wallet.network).is_ok_and(|bound| bound != network)
    }) {
        return Err(Error::usage("Wallet registration conflicts with --network"));
    }
    Ok(())
}

#[cfg(test)]
mod key_boundary_tests {
    use super::*;
    use serde_json::json;

    fn public_registration() -> Value {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .to_bytes();
        let public = bs58::encode(key).into_string();
        json!({
            "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
            "address": public,
            "publicKey": public,
            "keyType": "ed25519"
        })
    }

    #[test]
    fn setup_accepts_only_matching_public_wallet_metadata() {
        let public = public_registration();
        assert!(registration(&public).is_ok());
        let mut wrong = public.clone();
        wrong["address"] = json!("11111111111111111111111111111111");
        assert!(registration(&wrong).is_err());
        for field in [
            "seed",
            "privateKey",
            "keypair",
            "agentShare",
            "walletFile",
            "config",
        ] {
            let mut secret = public.clone();
            secret[field] = json!({"value": "private material"});
            assert!(registration(&secret).is_err());
        }
        assert!(registration(&json!({})).is_err());
    }

    #[test]
    fn public_registration_cannot_change_the_selected_network() {
        crate::routing::with_network(Some("tempo:testnet".into()), || {
            assert!(registration(&public_registration()).is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn whoami_refuses_any_input_field() {
        assert!(control::public_fields(&json!({}), &[]).is_ok());
        assert!(control::public_fields(&json!({"seed":"private"}), &[]).is_err());
    }
}
