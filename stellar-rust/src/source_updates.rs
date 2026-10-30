//! Owner-offline-signed full-source updates do not modify the native prefix or Grant.
use crate::*;
use allowit_policy_sdk::source_updates::{
    SourceUpdate, SourceUpdateBinding, SourceUpdateDisposition, initial_rule_prefix,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct VerifiedSourceUpdate {
    update: SourceUpdate,
    disposition: SourceUpdateDisposition,
    authority: AccountState,
    prefix_digest: [u8; 32],
}
impl VerifiedSourceUpdate {
    pub fn update(&self) -> &SourceUpdate {
        &self.update
    }
    pub fn disposition(&self) -> SourceUpdateDisposition {
        self.disposition
    }
    pub fn authority(&self) -> &AccountState {
        &self.authority
    }
    pub fn prefix_digest(&self) -> [u8; 32] {
        self.prefix_digest
    }
}

/// Route-aware source binding. A classic account has no standard contract prefix or Wasm.
pub fn source_binding_digest(config: &WalletConfig, rules: &[String]) -> Result<[u8; 32]> {
    if config.route == WalletRoute::PolicyWallet {
        return crate::prefix_digest(config, rules);
    }
    if config.route != WalletRoute::Native {
        return Err("Historical signed wallets do not accept source updates.".into());
    }
    native_source_binding_digest(
        &config.network,
        &config.wallet,
        config.policy_instance,
        &config.asset,
        config.max_fee_stroops,
    )
}
/// Native-account source identity has no Wasm or standard contract rule prefix.
pub fn native_source_binding_digest(
    network: &str,
    wallet: &str,
    policy_instance: [u8; 32],
    asset: &str,
    max_fee_stroops: u32,
) -> Result<[u8; 32]> {
    passphrase(network)?;
    account_key(wallet)?;
    if policy_instance == [0; 32]
        || !(100..=1_000_000).contains(&max_fee_stroops)
        || !matches!(address(asset)?, ScAddress::Contract(_))
    {
        return Err("Invalid native source binding identity".into());
    }
    let value = ScVal::Vec(Some(
        vec![
            value_symbol("allowit_native_source")?,
            ScVal::String(ScString(
                PROFILE.as_bytes().to_vec().try_into().map_err(err)?,
            )),
            ScVal::String(ScString(
                network.as_bytes().to_vec().try_into().map_err(err)?,
            )),
            ScVal::Address(address(wallet)?),
            value_bytes(&policy_instance)?,
            ScVal::Address(address(asset)?),
            value_bytes(&hash(ARGUMENT_SCHEMA))?,
            value_bytes(&hash(vaults::DEPOSIT_ARGUMENT_SCHEMA))?,
            ScVal::U32(max_fee_stroops),
        ]
        .try_into()
        .map_err(err)?,
    ));
    Ok(hash(value.to_xdr(limits()).map_err(err)?))
}

/// Read current owner authority. A transport session or saved signer list is not authorization.
pub fn source_update_binding(
    rpc: &dyn rpc::Rpc,
    config: &WalletConfig,
    current_revision: u64,
    current_source_hash: [u8; 32],
) -> Result<(SourceUpdateBinding, AccountState)> {
    validate_config(config)?;
    if rpc.call("getNetwork", serde_json::json!({}))?["passphrase"] != passphrase(&config.network)?
    {
        return Err("The source approval RPC network differs from the wallet.".into());
    }
    let current_rules = initial_rule_prefix(&config.policy_source).map_err(err)?;
    let expected = source_binding_digest(config, &current_rules)?;
    let owner = match config.route {
        WalletRoute::Native => config.wallet.clone(),
        WalletRoute::PolicyWallet => {
            let discovered = rpc::trusted_state_for_discovery(rpc, config, None)?;
            // Discovery checks exact installed module, native schemas and prefix binding.
            discovered.state.owner
        }
        WalletRoute::Odc => {
            return Err("Historical signed wallets do not accept new source approvals.".into());
        }
    };
    account_key(&owner).map_err(|_|"Offline source updates require a current G-account owner; custom contract-owner proofs are not supported.")?;
    let authority = rpc::get_account(rpc, &owner)?;
    authority.validate_structure()?;
    if authority.address != owner {
        return Err("The current source owner account differs from its RPC observation.".into());
    }
    Ok((
        SourceUpdateBinding {
            owner,
            network: config.network.clone(),
            wallet: config.wallet.clone(),
            policy_instance: hex(&match config.route {
                WalletRoute::PolicyWallet => config.source_instance.ok_or(
                    "Fresh Std source approval requires an explicit logical source instance.",
                )?,
                WalletRoute::Native => config.source_instance.unwrap_or(config.policy_instance),
                WalletRoute::Odc => {
                    return Err("Historical signed wallets do not accept source approvals.".into());
                }
            }),
            prefix_digest: hex(&expected),
            current_revision,
            current_source_hash: hex(&current_source_hash),
        },
        authority,
    ))
}

pub fn source_update_digest(update: &SourceUpdate) -> Result<[u8; 32]> {
    let message = update.signing_message().map_err(err)?;
    Ok(hash(format!("Stellar Signed Message:\n{message}")))
}

/// Create an offline SEP-53 message signature locally. No RPC or transaction is involved.
pub fn sign_source_update(update: &SourceUpdate, seed: &[u8; 32]) -> Result<DetachedSignature> {
    let digest = source_update_digest(update)?;
    let signer = SigningKey::from_bytes(seed);
    Ok(DetachedSignature {
        signer: account_for_seed(seed),
        signature: signer.sign(&digest).to_bytes().to_vec(),
    })
}

/// Verify exact bytes, unchanged native prefix, next revision and the current owner quorum.
pub fn verify_source_update(
    rpc: &dyn rpc::Rpc,
    config: &WalletConfig,
    update: &SourceUpdate,
    signatures: &[DetachedSignature],
    current_revision: u64,
    current_source_hash: [u8; 32],
) -> Result<VerifiedSourceUpdate> {
    let (binding, authority) =
        source_update_binding(rpc, config, current_revision, current_source_hash)?;
    let disposition = update.validate_binding(&binding).map_err(err)?;
    let rules = initial_rule_prefix(&update.source).map_err(err)?;
    let actual_prefix = source_binding_digest(config, &rules)?;
    if hex(&actual_prefix) != binding.prefix_digest || hex(&actual_prefix) != update.prefix_digest {
        return Err("An offline update cannot change the native custody binding or the standard contract's rule prefix.".into());
    }
    if signatures.is_empty() || signatures.len() > 21 {
        return Err("Provide the current owner account's bounded administrator quorum.".into());
    }
    let digest = source_update_digest(update)?;
    let mut distinct = BTreeSet::new();
    for signature in signatures {
        if !distinct.insert(signature.signer.as_str()) {
            return Err("An owner signer was repeated.".into());
        }
        let key = account_key(&signature.signer)?;
        let bytes: [u8; 64] = signature
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| "An owner signature must contain exactly 64 bytes.")?;
        VerifyingKey::from_bytes(&key)
            .map_err(err)?
            .verify_strict(&digest, &EdSignature::from_bytes(&bytes))
            .map_err(|_| "The owner source signature is invalid.")?;
    }
    authority.validate_signer_quorum(
        &distinct.into_iter().collect::<Vec<_>>(),
        authority.high_threshold,
    )?;
    Ok(VerifiedSourceUpdate {
        update: update.clone(),
        disposition,
        authority,
        prefix_digest: actual_prefix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct OwnerRpc {
        network: String,
        account: Mutex<AccountEntry>,
    }
    impl rpc::Rpc for OwnerRpc {
        fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
            match method {
                "getNetwork" => Ok(serde_json::json!({"passphrase":passphrase(&self.network)?})),
                "getLedgerEntries" => {
                    let key: LedgerKey =
                        read(params["keys"][0].as_str().ok_or("Missing owner key")?)?;
                    let LedgerKey::Account(key) = key else {
                        return Err("Only current owner account reads are allowed".into());
                    };
                    let account = self.account.lock().unwrap().clone();
                    if key.account_id != account.account_id {
                        return Err("Wrong owner account".into());
                    }
                    Ok(
                        serde_json::json!({"latestLedger":120,"entries":[{"xdr":xdr(&LedgerEntryData::Account(account))?}]}),
                    )
                }
                _ => Err("Source updates do not sign or submit transactions".into()),
            }
        }
    }
    fn setup() -> (WalletConfig, SourceUpdate, OwnerRpc) {
        let source = "async fn _execute(ctx:&Context)->PolicyResult { Ok(()) }";
        let mut config = WalletConfig {
            source_instance: None,
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: account_for_seed(&[1; 32]),
            owner: account_for_seed(&[1; 32]),
            agent: account_for_seed(&[8; 32]),
            server: account_for_seed(&[9; 32]),
            route: WalletRoute::Native,
            policy_engine: None,
            asset: String::new(),
            asset_symbol: "XLM".into(),
            asset_issuer: None,
            policy_source: source.into(),
            source_hash: hash(source),
            module_digest: [5; 32],
            max_fee_stroops: 100,
            prefix_digest: None,
        };
        config.asset = sac_address(&config.network, &asset_xdr(&config).unwrap()).unwrap();
        let prefix = source_binding_digest(&config, &[]).unwrap();
        let source = format!("{source}\n");
        let update = SourceUpdate {
            owner: config.wallet.clone(),
            network: config.network.clone(),
            wallet: config.wallet.clone(),
            policy_instance: hex(&config.source_instance.unwrap_or(config.policy_instance)),
            prefix_digest: hex(&prefix),
            source_hash: hex(&hash(source.as_bytes())),
            revision: 2,
            source,
        };
        let account = AccountEntry {
            account_id: AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(
                account_key(&config.wallet).unwrap(),
            ))),
            seq_num: SequenceNumber(40),
            thresholds: Thresholds([0, 1, 2, 3]),
            signers: vec![
                stellar_xdr::Signer {
                    key: SignerKey::Ed25519(Uint256(
                        account_key(&account_for_seed(&[2; 32])).unwrap(),
                    )),
                    weight: 2,
                },
                stellar_xdr::Signer {
                    key: SignerKey::Ed25519(Uint256(
                        account_key(&account_for_seed(&[3; 32])).unwrap(),
                    )),
                    weight: 1,
                },
            ]
            .try_into()
            .unwrap(),
            ..Default::default()
        };
        let rpc = OwnerRpc {
            network: config.network.clone(),
            account: Mutex::new(account),
        };
        (config, update, rpc)
    }
    #[test]
    fn current_admin_quorum_binds_exact_source_and_cannot_be_replayed_after_rotation() {
        let (config, update, rpc) = setup();
        let a = sign_source_update(&update, &[2; 32]).unwrap();
        let b = sign_source_update(&update, &[3; 32]).unwrap();
        assert!(
            verify_source_update(&rpc, &config, &update, &[a.clone()], 1, config.source_hash)
                .is_err()
        );
        assert!(
            verify_source_update(
                &rpc,
                &config,
                &update,
                &[a.clone(), a.clone()],
                1,
                config.source_hash
            )
            .is_err()
        );
        let verified = verify_source_update(
            &rpc,
            &config,
            &update,
            &[a.clone(), b.clone()],
            1,
            config.source_hash,
        )
        .unwrap();
        assert_eq!(verified.disposition(), SourceUpdateDisposition::Advance);
        let mut modified = update.clone();
        modified.source.push(' ');
        modified.source_hash = hex(&hash(modified.source.as_bytes()));
        assert!(
            verify_source_update(
                &rpc,
                &config,
                &modified,
                &[a.clone(), b.clone()],
                1,
                config.source_hash
            )
            .is_err()
        );
        rpc.account.lock().unwrap().signers = VecM::default();
        assert!(
            verify_source_update(&rpc, &config, &update, &[a, b], 1, config.source_hash).is_err()
        );
    }
    #[test]
    fn classic_account_updates_may_change_leading_calls_without_claiming_standard_preflight() {
        let (config, mut update, rpc) = setup();
        let source = "async fn _execute(ctx:&Context)->PolicyResult {allowit::spent_cap(grant::spent_cap)?; Ok(())}";
        update.source = source.into();
        update.source_hash = hex(&hash(source));
        let signatures = [
            sign_source_update(&update, &[2; 32]).unwrap(),
            sign_source_update(&update, &[3; 32]).unwrap(),
        ];
        assert!(
            verify_source_update(&rpc, &config, &update, &signatures, 1, config.source_hash)
                .is_ok()
        );
        let (config, update, rpc) = setup();
        let signatures = [
            sign_source_update(&update, &[2; 32]).unwrap(),
            sign_source_update(&update, &[3; 32]).unwrap(),
        ];
        assert!(
            verify_source_update(&rpc, &config, &update, &signatures, 2, config.source_hash)
                .is_err()
        );
    }
}
