//! `allowit policy SUBCOMMAND`: owner policy lifecycle. Validates arguments and
//! routes each rail to its vendor implementation under `vendors/`.
//!
//! Subcommands: `generate`, `import`, `deploy`, `fund`, `execute`, `status`,
//! `revoke`, `withdraw`, `close`, `tune`, `tune-action`. Every one accepts
//! `--json`. Solana (default `solana:testnet`) runs through the native SDK and
//! the chain; no `ALLOWIT_TOKEN` is needed. `--network stellar:testnet deploy`
//! approves an installed policy; `tempo:*` supports only `import` and `execute`.
//!
//! ```text
//! export ALLOWIT_POLICY_DIR=.allowit ALLOWIT_NETWORK=solana:testnet
//! allowit policy generate "Pay up to 5 USDC a day to approved merchants"
//! ALLOWIT_OWNER=OWNER_PUBKEY ALLOWIT_REQUEST_ID=deploy-0001 \
//!   allowit policy deploy 10            # owner signs in the browser
//! allowit policy import executor.json
//! ALLOWIT_REQUEST_ID=pay-0001 allowit policy execute TOKEN_ACCOUNT 0.25
//! allowit policy status --json
//! ```
use crate::{
    error::{Error, Result, quoted},
    request::{matches, solana_address},
};
const USAGE: &str = include_str!("usage.txt");
pub(crate) fn run(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if args.is_empty() {
        stderr.push_str(USAGE);
        return Ok(2);
    }
    let name = args[0].as_str();
    if ["help", "-h", "--help", "-help"].contains(&name) {
        stdout.push_str(USAGE);
        return Ok(0);
    }
    let parameters: &[&str] = match name {
        "generate" => &["PROMPT"],
        "import" => &["EXECUTOR_JSON"],
        "status" | "revoke" | "close" => &[],
        "deploy" if crate::routing::selected().is_some_and(|n| n.starts_with("stellar:")) => &[],
        "deploy" | "fund" | "withdraw" => &["AMOUNT"],
        "execute" => &["RECIPIENT", "AMOUNT"],
        "tune" | "tune-action" => &["VALUE"],
        _ => {
            return Err(Error::usage(format!(
                "unknown policy command {} (generate, import, deploy, fund, execute, status, revoke, withdraw, close, tune, tune-action)",
                quoted(name)
            )));
        }
    };
    let synopsis = format!(
        "allowit policy {name}{} [--json]",
        if parameters.is_empty() {
            String::new()
        } else {
            format!(" {}", parameters.join(" "))
        }
    );
    let mut pos = Vec::new();
    let mut json = false;
    let mut help = false;
    let mut literal = false;
    for a in &args[1..] {
        if literal {
            pos.push(a.clone());
            continue;
        }
        match a.as_str() {
            "--" => literal = true,
            "--json" | "-json" => json = true,
            "-h" | "--help" | "-help" => help = true,
            _ if a.starts_with('-') && a != "-" => {
                return Err(Error::usage(format!(
                    "unknown flag {a} (only --json is accepted; put -- before a PROMPT that begins with -)"
                )));
            }
            _ => pos.push(a.clone()),
        }
    }
    if help {
        stdout.push_str(&format!("usage: {synopsis}\n\n{USAGE}"));
        return Ok(0);
    }
    if pos.len() != parameters.len() {
        return Err(Error::usage(format!(
            "usage: {synopsis}{}",
            if name == "generate" && pos.len() > 1 {
                " (quote the prompt so it is one argument)"
            } else {
                ""
            }
        )));
    }
    if pos.iter().any(|p| p.contains('\0')) {
        return Err(Error::usage("arguments must not contain NUL characters"));
    }
    match name {
        "generate" if pos[0].trim().is_empty() => {
            return Err(Error::usage("PROMPT must not be empty"));
        }
        "deploy" if crate::routing::selected().is_some_and(|n| n.starts_with("stellar:")) => {}
        "deploy" | "fund" | "withdraw" => decimal("AMOUNT", &pos[0], true)?,
        "execute" => {
            if crate::routing::selected().is_none_or(|n| n.starts_with("solana:"))
                && !solana_address(&pos[0])
            {
                return Err(Error::usage(format!(
                    "RECIPIENT {} is not a Solana token account address",
                    quoted(&pos[0])
                )));
            }
            decimal("AMOUNT", &pos[1], true)?;
        }
        "tune" | "tune-action" => decimal("VALUE", &pos[0], false)?,
        _ => {}
    }
    if let Some(network) = crate::routing::selected() {
        if network.starts_with("stellar:") {
            if name != "deploy" {
                return Err(Error::unsupported(
                    "Use wallet configure for native setup, wallet approve for an existing installed Stellar policy, or exec/status with its public executor bundle",
                ));
            }
            let file = crate::config::env_value("ALLOWIT_STELLAR_POLICY_FILE");
            let owner = crate::config::env_value("ALLOWIT_OWNER");
            let request = crate::config::env_value("ALLOWIT_REQUEST_ID");
            if file.is_empty() || owner.is_empty() || request.is_empty() {
                return Err(Error::config(
                    "Stellar deployment approves an existing installed policy and moves no funds. Set ALLOWIT_STELLAR_POLICY_FILE, ALLOWIT_OWNER and ALLOWIT_REQUEST_ID, or use wallet approve --policy-file PATH --owner G_ADDRESS --request-id ID",
                ));
            }
            return crate::vendors::stellar::approval::run(
                &[
                    "--policy-file".into(),
                    file,
                    "--owner".into(),
                    owner,
                    "--request-id".into(),
                    request,
                ],
                stdout,
            );
        }
        if network.starts_with("tempo:") {
            if !matches!(name, "execute" | "import") {
                return Err(Error::unsupported(
                    "This Tempo lifecycle operation requires the owner browser; no Solana fallback was used",
                ));
            }
            let mut args = vec![if name == "execute" {
                "pay".into()
            } else {
                "import".into()
            }];
            args.extend(pos);
            if json {
                args.push("--json".into());
            }
            return crate::vendors::tempo::command(&args, stdout, stderr);
        }
        if !network.starts_with("solana:") {
            return Err(Error::unsupported(
                "Policy lifecycle for this rail requires its owner browser",
            ));
        }
    }
    crate::vendors::solana::policy::run(name, &pos, json, stdout, stderr)
}
fn decimal(name: &str, v: &str, positive: bool) -> Result<()> {
    if v.len() > 40
        || !matches(v, r"^(0|[1-9][0-9]*)(\.[0-9]+)?$")
        || positive && v.trim_matches(['0', '.']).is_empty()
    {
        return Err(Error::usage(format!(
            "{name} {} must be {} such as 5 or 0.25 (no sign, exponent or separator)",
            quoted(v),
            if positive {
                "a positive decimal"
            } else {
                "a non-negative decimal"
            }
        )));
    }
    Ok(())
}
