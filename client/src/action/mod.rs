//! Shared pipeline for the show, eval, exec and status policy commands.
mod poll;
mod submit;

pub(crate) use poll::{check_status, poll};
pub(crate) use submit::submit;

use crate::{
    args::{self, Args},
    client::Client,
    config::env_value,
    error::{Error, Result},
    request::request_id_valid,
    routing,
    skill::{Skill, text},
    typed_run,
};
use serde_json::Value;

/// The outcome of the shared preamble: a vendor route finished the command,
/// or the command continues against the Allowit policy API.
pub(crate) enum Begin {
    Done(i32),
    Policy(Args),
}

/// Parses and validates the command, then routes Tempo, Stellar and typed
/// requests to their own flows.
pub(crate) fn begin(
    command: &str,
    argv: &[String],
    stdout: &mut String,
    stderr: &mut String,
) -> Result<Begin> {
    let parsed = args::parse(command, argv)?;
    let expected = if command == "status" { 2 } else { 1 };
    if parsed.pos.len() != expected {
        return Err(Error::usage(match command {
            "show" => "usage: allowit show POLICY [--json] [--source]".into(),
            "status" => "usage: allowit status POLICY REQUEST_ID [--wait 60s]".into(),
            _ => format!(
                "usage: allowit {command} POLICY --action ACTION [--rail RAIL --op OP] --addr ADDRESS --amount QUANTITY [--context JSON]"
            ),
        }));
    }
    if parsed.negative_wait || parsed.wait > std::time::Duration::from_secs(600) {
        return Err(Error::usage("--wait must be between 0s and 10m0s"));
    }
    let policy = &parsed.pos[0];
    if let Some(network) = routing::selected()
        && network.starts_with("tempo:")
    {
        return crate::vendors::tempo::action(command, &parsed, stdout, stderr).map(Begin::Done);
    }

    if command == "status" && !request_id_valid(&parsed.pos[1]) {
        return Err(Error::usage(
            "REQUEST_ID is the requestId printed by eval or exec",
        ));
    }
    if env_value("ALLOWIT_TOKEN").starts_with("allowit-stellar-v1.")
        || !env_value("ALLOWIT_STELLAR_BUNDLE").is_empty()
    {
        return match command {
            "exec" | "status" => {
                crate::vendors::stellar::run::run(command, &parsed, stdout, stderr)
            }
            "show" => crate::vendors::stellar::run::show(policy, parsed.source, stdout),
            _ => Err(Error::unsupported(
                "Stellar evaluation requires its installed policy execution flow; use exec with a typed request file",
            )),
        }
        .map(Begin::Done);
    }
    if parsed.flags.0.contains_key("request-file") {
        return typed_run::exec(command, &parsed, stderr).map(Begin::Done);
    }
    let id_flag = parsed.flags.get("request-id");
    if !id_flag.is_empty() && !request_id_valid(id_flag) {
        return Err(Error::usage(
            "--request-id must be 8 to 100 characters from A-Z a-z 0-9 . _ : -",
        ));
    }
    Ok(Begin::Policy(parsed))
}

/// Reads the policy description and checks it against the selected network.
pub(crate) fn read_skill(c: &mut Client) -> Result<Skill> {
    let s = Skill::read(c)?;
    if let Some(network) = routing::selected()
        && routing::normalize(s.get("network"))? != network
    {
        return Err(Error::config(
            "Selected network differs from the policy deployment",
        ));
    }
    Ok(s)
}

pub(crate) fn unknown(e: Error, kind: &str, client_id: &str, policy: &str, r: &Value) -> Error {
    let server_id = text(&r["requestId"]);
    let (label, next) = if request_id_valid(server_id) {
        (
            format!(" (server requestId {server_id})"),
            format!("allowit status --wait 60s -- {policy} {server_id}, or "),
        )
    } else {
        (String::new(), String::new())
    };
    Error::uncertain(format!(
        "{e}\nAllowit accepted {kind} request {client_id}{label}, but its result is unknown. Do not retry with a new request ID. Check it with {next}rerun the same command with --request-id {client_id}"
    ))
}
