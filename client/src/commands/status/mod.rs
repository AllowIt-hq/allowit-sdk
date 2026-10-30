//! `allowit status POLICY REQUEST_ID [--wait 60s] [--json]`: reads one request
//! without creating another. Posts to `/api/harness/OWNER/POLICY/status` by
//! server `requestId`, then by the client `--request-id`. `--wait` polls a
//! pending request (default 0s, at most 10m). Tempo and installed Stellar
//! wallets read their own journals and endpoints.
//!
//! ```text
//! allowit exec POLICY --action transfer --rail solana --op transferUSDC \
//!   --addr SOLANA_ADDRESS --amount 1 --request-id exec-0003
//! # exit 12 (pending) or 5 (uncertain)
//! allowit status POLICY exec-0003 --wait 60s
//! allowit status POLICY SERVER_REQUEST_ID --json
//! ```
use crate::{
    action::{self, Begin, check_status, poll},
    args,
    client::Client,
    config::Config,
    error::{self, Error, Result},
    output::{self, classify},
    request::encode,
    routing,
    skill::{self, Network, Skill, text},
};
use serde_json::json;

pub(crate) fn run(argv: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    let parsed = match action::begin("status", argv, stdout, stderr)? {
        Begin::Done(code) => return Ok(code),
        Begin::Policy(parsed) => parsed,
    };
    let mut c = Client::new(Config::load(&parsed.pos[0])?)?;
    if let Some(network) = routing::selected() {
        let skill = Skill::read(&mut c)?;
        if routing::normalize(skill.get("network"))? != network {
            return Err(Error::config(
                "Selected network differs from policy deployment",
            ));
        }
    }
    status(&mut c, &parsed, stdout, stderr)
}

fn status(
    c: &mut Client,
    args: &args::Args,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let id = &args.pos[1];
    let net = skill::status_network(c, stderr)?;
    let mut r = match c.call("POST", "status", Some(&encode(&json!({"requestId":id})))) {
        Ok(r) => r,
        Err(e) if e.status == Some(404) => match c.call(
            "POST",
            "status",
            Some(&encode(&json!({"clientRequestId":id}))),
        ) {
            Err(e) if e.status == Some(404) => {
                return Err(Error::uncertain(format!(
                    "Allowit has no request {id} for this policy yet. If an exec with --request-id {id} may have been sent, rerun that identical command: Allowit applies a request ID at most once"
                )));
            }
            x => x?,
        },
        Err(e) => return Err(e),
    };
    let map_error = |e: Error| {
        Error::uncertain(format!(
            "{e}\nThe state of request {id} is unknown; check again with allowit status"
        ))
    };
    check_status(&r, id, net).map_err(map_error)?;
    let outcome = text(&r["outcome"]);
    if outcome == "pending"
        || (outcome == "awaiting_input" || ["submitted", "ready"].contains(&text(&r["status"])))
            && !args.wait.is_zero()
    {
        r = poll(c, text(&r["requestId"]), args.wait, &r, "status", net).map_err(map_error)?;
    }
    if net == Network::Unknown && output::needs_network(&r) {
        return Err(Error::uncertain(format!(
            "Allowit reported outcome {} with status {} for request {id}, but the policy's network could not be established, so the CLI cannot tell a Local dev mock recording from a wallet transfer. Nothing is confirmed as recorded, signed or paid. Check again with allowit status once allowit show works",
            error::quoted(text(&r["outcome"])),
            error::quoted(text(&r["status"]))
        )));
    }
    let res = classify(&r, "status");
    if args.json {
        r["state"] = json!(res.state);
        r["exitCode"] = json!(res.exit);
        *stdout += &output::json(&r);
    } else {
        *stdout += &output::print_result(&r, &res, "");
    }
    Ok(res.exit)
}
