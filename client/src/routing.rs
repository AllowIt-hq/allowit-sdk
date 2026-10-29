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
    Ok((rest, network))
}
pub(crate) fn tempo_action(
    command: &str,
    parsed: &crate::args::Args,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let rail = parsed.flags.get("rail");
    let op = parsed.flags.get("op");
    if (!rail.is_empty() && rail != "tempo") || (!op.is_empty() && op != "transferUSDC") {
        return Err(Error::usage(
            "Request rail or operation conflicts with Tempo token transfer",
        ));
    }
    let path = crate::config::env_value("ALLOWIT_TEMPO_BUNDLE");
    let path = if path.is_empty() {
        ".allowit-tempo/executor.json"
    } else {
        &path
    };
    let bundle: serde_json::Value =
        crate::typed_request::read_request_file(std::path::Path::new(path))?;
    let network = bundle["policy"]["network"]
        .as_str()
        .ok_or_else(|| Error::config("Tempo bundle lacks its network"))?;
    if selected().as_deref() != Some(normalize(network)?) {
        return Err(Error::config(
            "Selected network differs from the Tempo deployment",
        ));
    }
    if bundle["policy"]["id"].as_str() != Some(parsed.pos[0].as_str()) {
        return Err(Error::config("POLICY differs from the Tempo bundle"));
    }
    if command == "show" {
        let policy = allowit_tempo::describe(std::path::Path::new(path))?;
        *stdout += &crate::output::json(&policy);
        return Ok(0);
    }
    let mut args = match command {
        "status" => vec!["status".into(), parsed.pos[1].clone()],
        "exec" | "eval" => {
            if !parsed.flags.get("action").is_empty() && parsed.flags.get("action") != "transfer" {
                return Err(Error::unsupported(
                    "Tempo generic execution requires action transfer",
                ));
            }
            for name in [
                "context",
                "merchant",
                "before",
                "after",
                "data",
                "memo",
                "request-file",
                "budget",
            ] {
                if !parsed.flags.get(name).is_empty() {
                    return Err(Error::unsupported(
                        "Use Tempo's bound execution context and ALLOWIT_REQUEST_ID; generic request overrides are refused",
                    ));
                }
            }
            vec![
                if command == "exec" {
                    "pay".into()
                } else {
                    "execute".into()
                },
                parsed.flags.get("addr").into(),
                parsed.flags.get("amount").into(),
            ]
        }
        _ => {
            return Err(Error::unsupported(
                "Use the owner browser to inspect Tempo policy state",
            ));
        }
    };
    if parsed.json {
        args.push("--json".into());
    }
    let request_id = parsed.flags.get("request-id");
    allowit_tempo::run_with_request_id(
        &args,
        if request_id.is_empty() {
            None
        } else {
            Some(request_id)
        },
        stdout,
        stderr,
    )
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

#[cfg(test)]
mod request_boundary_tests {
    use super::*;
    #[test]
    fn conflicting_tempo_rail_or_asset_fails_before_reading_bundle() {
        for flags in [
            vec!["--rail", "solana", "--op", "transferSOL"],
            vec!["--rail", "tempo", "--op", "transferXLM"],
        ] {
            let mut args = vec![
                "policy-id".to_string(),
                "--addr".into(),
                "recipient".into(),
                "--amount".into(),
                "1".into(),
            ];
            args.extend(flags.into_iter().map(str::to_owned));
            let parsed = crate::args::parse("exec", &args).unwrap();
            let mut stdout = String::new();
            let mut stderr = String::new();
            let error = tempo_action("exec", &parsed, &mut stdout, &mut stderr).unwrap_err();
            assert_eq!(error.code, 2);
            assert!(stdout.is_empty() && stderr.is_empty());
        }
    }
}
