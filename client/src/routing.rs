//! Explicit network selection never changes process environment or inferred authority.
use crate::error::{Error, Result};
use std::cell::RefCell;
thread_local! { static SELECTED: RefCell<Option<String>> = const { RefCell::new(None) }; }
pub fn normalize(network: &str) -> Result<&'static str> {
    match network {
        "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1" | "solana:devnet" => Ok("solana:devnet"),
        "solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z" | "solana:testnet" => Ok("solana:testnet"),
        "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp" | "solana:mainnet" => Ok("solana:mainnet"),
        "eip155:42431" | "tempo:testnet" => Ok("tempo:testnet"),
        "eip155:4217" | "tempo:mainnet" => Ok("tempo:mainnet"),
        "stellar:pubnet" | "stellar:mainnet" => Ok("stellar:mainnet"),
        "stellar:testnet" => Ok("stellar:testnet"),
        _ => Err(Error::usage(
            "Unsupported network identifier; use an admitted x402 CAIP-2 network",
        )),
    }
}
pub fn caip2(network: &str) -> Result<&'static str> {
    match normalize(network)? {
        "solana:devnet" => Ok("solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1"),
        "solana:testnet" => Ok("solana:4uhcVJyU9pJkvQyS88uRDiswHXSCkY3z"),
        "solana:mainnet" => Ok("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
        "tempo:testnet" => Ok("eip155:42431"),
        "tempo:mainnet" => Ok("eip155:4217"),
        "stellar:mainnet" => Ok("stellar:pubnet"),
        _ => Ok("stellar:testnet"),
    }
}
pub(crate) fn selected() -> Option<String> {
    SELECTED.with(|n| n.borrow().clone())
}
pub(crate) fn with_network<T>(network: Option<String>, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SELECTED.with(|n| *n.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(SELECTED.with(|n| n.replace(network)));
    f()
}
pub(crate) fn extract(args: &[String]) -> Result<(Vec<String>, Option<String>)> {
    let mut rest = Vec::new();
    let mut network = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            rest.extend_from_slice(&args[i..]);
            break;
        }
        let value = if args[i] == "--network" {
            i += 1;
            Some(
                args.get(i)
                    .ok_or_else(|| Error::usage("--network requires an identifier"))?
                    .clone(),
            )
        } else {
            args[i].strip_prefix("--network=").map(str::to_owned)
        };
        if let Some(value) = value {
            if network.is_some() {
                return Err(Error::usage("--network may occur only once"));
            }
            network = Some(normalize(&value)?.to_owned());
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    if rest
        .iter()
        .take_while(|s| s.as_str() != "--")
        .any(|s| matches!(s.as_str(), "-h" | "--help" | "-help"))
        || rest.first().is_none_or(|s| {
            matches!(
                s.as_str(),
                "help" | "-h" | "--help" | "version" | "--version"
            )
        })
    {
        return Ok((rest, network));
    }
    if let Some(n) = &network {
        let bound = allowit_client_core::config::env_value("ALLOWIT_NETWORK");
        if !bound.is_empty() && normalize(&bound)? != n {
            return Err(Error::config("--network conflicts with ALLOWIT_NETWORK"));
        }
    }
    if network.is_none() {
        let configured = allowit_client_core::config::env_value("ALLOWIT_NETWORK");
        if !configured.is_empty() {
            network = Some(normalize(&configured)?.to_owned());
        } else if !allowit_client_core::config::env_value("ALLOWIT_TEMPO_BUNDLE").is_empty() {
            let path = allowit_client_core::config::env_value("ALLOWIT_TEMPO_BUNDLE");
            let bundle: serde_json::Value =
                crate::typed_request::read_request_file(std::path::Path::new(&path))?;
            let bound = bundle["policy"]["network"]
                .as_str()
                .ok_or_else(|| Error::config("Tempo bundle lacks network"))?;
            network = Some(normalize(bound)?.to_owned());
        }
    }
    let deployment_command = rest.first().is_some_and(|name| {
        matches!(
            name.as_str(),
            "show" | "eval" | "exec" | "status" | "policy"
        )
    }) && !rest
        .get(1)
        .is_some_and(|name| matches!(name.as_str(), "help" | "-h" | "--help" | "-help"));
    let stellar = allowit_client_core::config::env_value("ALLOWIT_STELLAR_BUNDLE");
    if deployment_command && !stellar.is_empty() {
        let (_, bound) =
            crate::vendors::stellar::run::public_binding(std::path::Path::new(&stellar))?;
        if network.as_ref().is_some_and(|selected| selected != &bound) {
            return Err(Error::config(
                "Selected network conflicts with the public Stellar wallet binding",
            ));
        }
        network = Some(bound);
    }
    if network.is_none()
        && rest
            .first()
            .is_some_and(|s| matches!(s.as_str(), "show" | "eval" | "exec" | "status"))
    {
        let parsed = crate::args::parse(&rest[0], &rest[1..])?;
        if let Some(selector) = parsed.pos.first() {
            let path = std::path::Path::new(".allowit-tempo/executor.json");
            if path.exists() {
                let policy = allowit_tempo::describe(path)?;
                if policy["id"].as_str() == Some(selector.as_str()) {
                    let token = allowit_client_core::config::env_value("ALLOWIT_TOKEN");
                    let directory = allowit_client_core::config::env_value("ALLOWIT_POLICY_DIR");
                    let context = std::path::Path::new(if directory.is_empty() {
                        ".allowit"
                    } else {
                        &directory
                    })
                    .join("context.json");
                    let native_matches = if context.exists() {
                        let c: serde_json::Value =
                            crate::typed_request::read_request_file(&context)?;
                        c["policyId"].as_str() == Some(selector.as_str())
                    } else {
                        false
                    };
                    if native_matches || token.starts_with("allowit-stellar-v1.") {
                        return Err(Error::config(
                            "Multiple wallet bindings match this operation; select --network explicitly",
                        ));
                    }
                    network = Some(
                        normalize(policy["network"].as_str().ok_or_else(|| {
                            Error::config("Imported Tempo policy lacks network")
                        })?)?
                        .to_owned(),
                    );
                }
            }
        }
    }
    Ok((rest, network))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_networks_and_conflicts() {
        assert_eq!(normalize("eip155:42431").unwrap(), "tempo:testnet");
        assert!(normalize("eip155:1").is_err());
        assert!(
            extract(&[
                "--network=eip155:42431".into(),
                "--network=eip155:4217".into()
            ])
            .is_err()
        );
    }
    #[test]
    fn selection_is_scoped() {
        with_network(Some("tempo:testnet".into()), || {
            assert_eq!(selected().as_deref(), Some("tempo:testnet"));
            Ok(())
        })
        .unwrap();
        assert!(selected().is_none());
    }
}
