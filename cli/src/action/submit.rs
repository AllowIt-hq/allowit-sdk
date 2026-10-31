//! Submission shared by eval and exec. Derives or checks the request ID, prints
//! recovery instructions before the POST, posts once to `judge` (eval) or
//! `transactions` (exec), then polls status within `--wait`.
use super::{Begin, begin, poll, read_skill, unknown};
use crate::{
    client::Client,
    config::Config,
    error::{Error, Result},
    output::{self, check_result, classify},
    request::{self, encode, request_id_valid},
    skill::text,
};
use serde_json::{Value, json};

/// Evaluates (`eval`) or submits (`exec`) one request through the policy.
pub(crate) fn submit(
    command: &str,
    argv: &[String],
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let parsed = match begin(command, argv, stdout, stderr)? {
        Begin::Done(code) => return Ok(code),
        Begin::Policy(parsed) => parsed,
    };
    let policy = &parsed.pos[0];
    let id_flag = parsed.flags.get("request-id");
    let mut c = Client::new(Config::load(policy)?)?;
    let s = read_skill(&mut c)?;
    let mut b = request::build_body(command, &parsed.flags, &s, &mut std::io::stdin().lock())?;
    if !parsed.flags.get("budget").is_empty() {
        let want = request::usdc_units(parsed.flags.get("budget"))
            .map_err(|e| Error::usage(format!("--budget: {e}")))?;
        if request::usdc_units(&b.amount).ok() != Some(want) {
            return Err(Error::usage(format!(
                "--budget {} does not match the computed charge of {} USDC",
                parsed.flags.get("budget"),
                b.amount
            )));
        }
    }
    b.request_id = id_flag.into();
    if b.request_id.is_empty() {
        b.request_id = request::derive_id(command, &c.cfg, &mut b);
    }
    *stderr += &format!(
        "allowit: {command} request {} (budget charge {} USDC)\n",
        b.request_id, b.amount
    );
    if command == "exec" {
        *stderr += &format!(
            "allowit: if the result is unknown, retry only with --request-id {}\n",
            b.request_id
        );
    }
    // Flush recovery instructions before a POST can leave the process.
    crate::flush_stderr(stderr);
    let mut r = match c.call(
        "POST",
        if command == "exec" {
            "transactions"
        } else {
            "judge"
        },
        Some(&encode(&b)),
    ) {
        Ok(r) => r,
        Err(e) => {
            if e.code == 5 {
                return Err(Error::uncertain(format!(
                    "{e}\nThe request may have reached Allowit. Do not retry with a new request ID. Retry only by rerunning the same command with --request-id {}: Allowit applies a request ID at most once",
                    b.request_id
                )));
            }
            if e.status == Some(409) {
                return Err(Error::uncertain(format!(
                    "{e}\nA request with --request-id {} already exists with different details and may have been applied. Read it with: allowit status --wait 0s -- {policy} {}\nUse a new --request-id only for a different intended operation",
                    b.request_id, b.request_id
                )));
            }
            return Err(e);
        }
    };
    let resent = c.resent;
    let replayed = r["replayed"] == true;
    let created = r.get("createdAt").cloned();
    check_result(&r, command, s.net())
        .map_err(|e| unknown(e, command, &b.request_id, policy, &r))?;
    if text(&r["outcome"]) == "pending" {
        let id = text(&r["requestId"]).to_string();
        if !request_id_valid(&id) {
            return Err(unknown(
                Error::uncertain("Allowit returned a pending result without a valid requestId"),
                command,
                &b.request_id,
                policy,
                &r,
            ));
        }
        r = poll(&mut c, &id, parsed.wait, &r, command, s.net())
            .map_err(|e| unknown(e, command, &b.request_id, policy, &r))?;
    }
    if replayed {
        r["replayed"] = Value::Bool(true);
        if let Some(v) = created {
            r["createdAt"] = v;
        }
    }
    let mut res = classify(&r, command);
    if r["replayed"] == true {
        if id_flag.is_empty() && !resent {
            r["replayedState"] = json!(res.state);
            res = output::ResultState {
                state: "replayed".into(),
                exit: 6,
                note: format!(
                    "An identical earlier request (derived request ID {}) already has this result; nothing new was submitted. Its state was {}. For another intended operation pass a new --request-id.",
                    b.request_id, res.state
                ),
            };
        } else {
            res.note += " This is the stored result of an earlier request with this --request-id; nothing new was submitted.";
        }
    }
    if parsed.json {
        r["state"] = json!(res.state);
        r["clientRequestId"] = json!(b.request_id);
        r["budgetChargeUSDC"] = json!(b.amount);
        r["exitCode"] = json!(res.exit);
        *stdout += &output::json(&r);
    } else {
        *stdout += &output::print_result(&r, &res, &b.request_id);
        *stdout += &format!("budgetChargeUSDC: {}\n", b.amount);
    }
    Ok(res.exit)
}
