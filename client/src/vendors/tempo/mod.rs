//! Tempo connection: the pinned Tempo crate owns execution and paid calls.
//! The selected network must match the Tempo bundle before any call.
//! Backs `allowit tempo`, `allowit policy import|execute --network tempo:*` and
//! `show`/`eval`/`exec`/`status` with a Tempo network.
use crate::{
    error::{Error, Result},
    routing::{normalize, selected},
};

pub(crate) fn action(
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
fn import_path(args: &[String]) -> Result<&str> {
    let mut literal = false;
    for value in &args[1..] {
        if !literal && value == "--" {
            literal = true;
            continue;
        }
        if !literal && value == "--json" {
            continue;
        }
        return Ok(value);
    }
    Err(Error::usage("Tempo import requires bundle path"))
}
pub(crate) fn command(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if let Some(selected) = selected() {
        if args
            .first()
            .is_some_and(|s| matches!(s.as_str(), "help" | "-h" | "--help"))
        {
            return allowit_tempo::run(args, stdout, stderr);
        }
        let configured = crate::config::env_value("ALLOWIT_TEMPO_BUNDLE");
        let path = if args.first().is_some_and(|s| s == "import") {
            import_path(args)?
        } else if configured.is_empty() {
            ".allowit-tempo/executor.json"
        } else {
            &configured
        };
        let policy = allowit_tempo::describe(std::path::Path::new(path))?;
        let network = policy["network"]
            .as_str()
            .ok_or_else(|| Error::config("Tempo policy lacks network"))?;
        if normalize(network)? != selected {
            return Err(Error::config(
                "Selected network differs from the Tempo wallet deployment",
            ));
        }
    }
    allowit_tempo::run(args, stdout, stderr)
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
            let error = action("exec", &parsed, &mut stdout, &mut stderr).unwrap_err();
            assert_eq!(error.code, 2);
            assert!(stdout.is_empty() && stderr.is_empty());
        }
    }
}

#[cfg(test)]
mod import_tests {
    #[test]
    fn import_path_preserves_vendor_option_positions() {
        for args in [
            vec!["import", "--json", "bundle.json"],
            vec!["import", "bundle.json", "--json"],
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(super::import_path(&args).unwrap(), "bundle.json");
        }
        let args = vec!["import".into(), "--".into(), "--json".into()];
        assert_eq!(super::import_path(&args).unwrap(), "--json");
    }
}
