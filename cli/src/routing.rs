//! Explicit network selection for one command. `--network CAIP2` is extracted
//! before dispatch and may appear anywhere before `--`.
pub use allowit_sdk_engine::routing::*;

use crate::error::{Error, Result};

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
        let bound = allowit_sdk::client::config::env_value("ALLOWIT_NETWORK");
        if !bound.is_empty() && normalize(&bound)? != n {
            return Err(Error::config("--network conflicts with ALLOWIT_NETWORK"));
        }
    }
    if network.is_none() {
        let configured = allowit_sdk::client::config::env_value("ALLOWIT_NETWORK");
        if !configured.is_empty() {
            network = Some(normalize(&configured)?.to_owned());
        } else if !allowit_sdk::client::config::env_value("ALLOWIT_TEMPO_BUNDLE").is_empty() {
            let path = allowit_sdk::client::config::env_value("ALLOWIT_TEMPO_BUNDLE");
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
    let stellar = allowit_sdk::client::config::env_value("ALLOWIT_STELLAR_BUNDLE");
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
                    let token = allowit_sdk::client::config::env_value("ALLOWIT_TOKEN");
                    let directory = allowit_sdk::client::config::env_value("ALLOWIT_POLICY_DIR");
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
}
