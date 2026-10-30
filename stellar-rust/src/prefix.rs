//! Standard native prefix identity is independent of owner-signed offchain source revisions.
use crate::*;
pub fn prefix_digest(config: &WalletConfig, rules: &[String]) -> Result<[u8; 32]> {
    let kind = config
        .policy_engine
        .ok_or("A standard policy-wallet engine is required for a native prefix")?;
    if kind == PolicyEngine::CustomPolicy {
        return Err("Custom native compilation uses a separate complete-source binding".into());
    }
    if rules
        .iter()
        .any(|rule| !allowit_policy_sdk::rule_values::RULES.contains(&rule.as_str()))
    {
        return Err("Unknown canonical rule in standard prefix".into());
    }
    if kind == PolicyEngine::SimplePolicyEngine && !rules.is_empty() {
        return Err("Simple policy wallets cannot drop a nonempty standard prefix".into());
    }
    let pairs = rules
        .iter()
        .map(|rule| {
            Ok(ScVal::Vec(Some(
                vec![value_symbol(rule)?, value_symbol(rule)?]
                    .try_into()
                    .map_err(err)?,
            )))
        })
        .collect::<Result<Vec<_>>>()?;
    let value = ScVal::Vec(Some(
        vec![
            value_symbol("allowit_std_prefix")?,
            value_symbol(kind.symbol())?,
            value_bytes(&config.module_digest)?,
            ScVal::Address(address(&config.asset)?),
            value_bytes(&hash(ARGUMENT_SCHEMA))?,
            value_bytes(&hash(vaults::DEPOSIT_ARGUMENT_SCHEMA))?,
            ScVal::Vec(Some(pairs.try_into().map_err(err)?)),
        ]
        .try_into()
        .map_err(err)?,
    ));
    Ok(hash(value.to_xdr(limits()).map_err(err)?))
}
pub(crate) fn native_policy_digest(config: &WalletConfig) -> Result<[u8; 32]> {
    if config.route != WalletRoute::PolicyWallet
        || config.policy_engine == Some(PolicyEngine::CustomPolicy)
    {
        return Ok(config.source_hash);
    }
    let rules = allowit_policy_sdk::source_updates::initial_rule_prefix(&config.policy_source)
        .map_err(err)?;
    let actual = prefix_digest(config, &rules)?;
    if config.prefix_digest != Some(actual) {
        return Err(
            "Owner-approved standard prefix binding differs from source, module, asset or schemas"
                .into(),
        );
    }
    Ok(actual)
}
pub(crate) fn retained_binding(config: &WalletConfig) -> [u8; 32] {
    if config.route == WalletRoute::PolicyWallet {
        config.prefix_digest.unwrap_or(config.source_hash)
    } else {
        config.source_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_prefix_xdr_matches_independent_stellar_sdk_vector() {
        let source = "use allowit::prelude::*; async fn _execute(ctx:&Context)->PolicyResult {allowit::spent_cap(grant::spent_cap)?;allowit::recipients(grant::recipients)?;Ok(())}";
        let mut config = WalletConfig {
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: Contract([4; 32]).to_string(),
            owner: account_for_seed(&[1; 32]),
            agent: account_for_seed(&[2; 32]),
            server: account_for_seed(&[3; 32]),
            route: WalletRoute::PolicyWallet,
            policy_engine: Some(PolicyEngine::CommonPolicyEngine),
            asset: Contract([2; 32]).to_string(),
            asset_symbol: "XLM".into(),
            asset_issuer: None,
            policy_source: source.into(),
            source_hash: hash(source),
            prefix_digest: None,
            module_digest: [1; 32],
            max_fee_stroops: 1000,
        };
        let rules = vec!["spent_cap".into(), "recipients".into()];
        let digest = prefix_digest(&config, &rules).unwrap();
        assert_eq!(
            hex(&digest),
            "98e919009d9d49b876a8e48d93fb43203aa274bd1153a2980584db4c37a3339f"
        );
        config.prefix_digest = Some(digest);
        assert_eq!(native_policy_digest(&config).unwrap(), digest);
        config.policy_source=config.policy_source.replace("Ok(())","if ctx.amount_units==0{return Err(PolicyError{code:\"ZERO\".into(),reason:\"Positive amount required.\".into()});} Ok(())");
        config.source_hash = hash(&config.policy_source);
        assert_eq!(native_policy_digest(&config).unwrap(), digest);
        let reversed = vec!["recipients".into(), "spent_cap".into()];
        assert_ne!(prefix_digest(&config, &reversed).unwrap(), digest);
    }
}
