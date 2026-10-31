//! Owner operations (fund, withdraw, revoke, close, tune, tune-action) and
//! executor payments (execute) submitted through the native lifecycle.
//! Owner operations sign with `ALLOWIT_OWNER_KEYPAIR`; execute signs with
//! `ALLOWIT_EXECUTOR_KEYPAIR` and needs `ALLOWIT_REQUEST_ID`.
//!
//! ```text
//! allowit policy fund 10
//! ALLOWIT_REQUEST_ID=pay-0001 allowit policy execute RECIPIENT 0.25
//! allowit policy tune 20
//! allowit policy withdraw 5
//! allowit policy close
//! ```
use super::*;

pub(super) fn run(
    session: &Session,
    name: &str,
    pos: &[String],
    json: bool,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let Session {
        local,
        policy,
        owner,
        signer,
        client,
        audit,
        journal,
        lifecycle,
    } = *session;
    let request_id = env_value("ALLOWIT_REQUEST_ID");
    let saved_request_id = std::env::var_os("ALLOWIT_REQUEST_ID")
        .is_some()
        .then_some(request_id.as_str());
    let options = match name {
        "deploy" => Options {
            amount: Some(pos[0].clone()),
            ..Options::default()
        },
        "fund" | "withdraw" => Options {
            amount: Some(pos[0].clone()),
            additional_owner_operation: env_value("ALLOWIT_ADDITIONAL_OWNER_OPERATION") == "1",
            ..Options::default()
        },
        "tune" => Options {
            amount: Some(pos[0].clone()),
            ..Options::default()
        },
        "tune-action" => Options {
            amount: Some(pos[0].clone()),
            ..Options::default()
        },
        "close" => native(lifecycle.close_options(policy, owner, saved_request_id))?,
        _ => Options::default(),
    };
    let method = if name == "tune-action" {
        "tune_action"
    } else {
        name
    };
    let mut result = if name == "execute" {
        if std::env::var_os("ALLOWIT_REQUEST_ID").is_none() || request_id.is_empty() {
            return Err(Error::config(
                "Set one fresh ALLOWIT_REQUEST_ID for this intended payment; keep it unchanged for retries",
            ));
        }
        let reporter = audit.as_ref().ok_or_else(|| {
            Error::config(
                "Import an owner-issued native executor bundle with its scoped approval capability",
            )
        })?;
        // Wallet recipients pay their associated token account, as the engine binds it.
        let requested_recipient = native(Key::parse(&pos[0]))?;
        let recipient = native(execution_recipient(client, requested_recipient))?;
        let action = configured("ALLOWIT_EXECUTION_ACTION", "transfer");
        let merchant = env_value("ALLOWIT_EXECUTION_MERCHANT");
        let context = execution_context()?;
        let identity = native(ExecutionRequestIdentity::new(
            policy,
            &request_id,
            &pos[1],
            recipient,
            &action,
            &merchant,
            &context,
        ))?;
        let executor = load_signer("ALLOWIT_EXECUTOR_KEYPAIR")?;
        native(lifecycle.submit_authorized(
            policy,
            owner,
            &identity,
            || reporter.authorize(&identity, requested_recipient, &context),
            |transaction| {
                if transaction.payer != executor.public_key() {
                    return Err(allowit_native::error::Error::config(
                        "Configured executor does not match the prepared fee payer",
                    ));
                }
                Ok(executor.sign(&transaction.message))
            },
        ))?
    } else {
        native(lifecycle.submit(
            policy,
            owner,
            method,
            &options,
            saved_request_id,
            |tx, role| {
                if role != "owner" {
                    return Err(allowit_native::error::Error::config(
                        "Owner operation requested an unexpected signer",
                    ));
                }
                let signer = signer.as_ref().ok_or_else(|| {
                    allowit_native::error::Error::config("Owner signing is unavailable")
                })?;
                if tx.payer != signer.public_key() {
                    return Err(allowit_native::error::Error::config(
                        "Configured signer does not match the prepared fee payer",
                    ));
                }
                Ok(signer.sign(&tx.message))
            },
        ))?
    };
    let replayed = result.extra.get("replayed") == Some(&serde_json::json!(true));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !result.final_status()
        && !result.expired()
        && std::time::Instant::now() < until
        && !audit
            .as_ref()
            .is_some_and(|a| a.blocked.load(std::sync::atomic::Ordering::Relaxed))
    {
        std::thread::sleep(std::time::Duration::from_secs(1));
        match lifecycle.recover(&result.id, policy, owner) {
            Ok(observed) => result = observed,
            Err(_) => {
                // The signed proof is already durable and may have landed.
                // A failed observation cannot turn it into a config-only exit.
                let name = format!("request-{}", result.id);
                match journal.locked(|| {
                    let mut current = journal.read::<Record>(&name)?.ok_or_else(|| allowit_native::error::Error::config("Operation journal disappeared"))?;
                    if current.signature != result.signature || current.signed_bytes != result.signed_bytes || current.intent != result.intent {
                        return Err(allowit_native::error::Error::config("Operation journal changed"));
                    }
                    if !current.final_status() && !current.expired() {
                        current.status = "uncertain".into();
                        current.extra.remove("replayed");
                        current.extra.insert("error".into(),serde_json::json!("Recovery observation failed; keep this request ID and original journal, and run allowit policy status"));
                        journal.write(&name,&current)?;
                    }
                    Ok(current)
                }) {
                    Ok(current) => result = current,
                    Err(_) => {
                        result.status = "uncertain".into();
                        stderr.push_str("Recovery observation could not be saved; retain the original journal and request ID.\n");
                    }
                }
                break;
            }
        }
    }
    if replayed {
        result
            .extra
            .insert("replayed".into(), serde_json::json!(true));
    }
    let mut output = result.public();
    let (broadcast_withheld, audit_pending) = if name == "execute" {
        audit
            .as_ref()
            .map(|a| a.finish(journal, &result.id))
            .unwrap_or((false, false))
    } else {
        (false, false)
    };
    if audit.is_some() && name == "execute" {
        if broadcast_withheld {
            output["broadcastWithheld"] = serde_json::json!(true);
        }
        output["auditStatus"] = serde_json::json!(if audit_pending {
            "pending"
        } else {
            "confirmed"
        });
    }
    if result.status == "settled" && name == "deploy" {
        let state = native(client.state(policy, owner, true, None))?
            .ok_or_else(|| Error::config("Deployed vault is unavailable"))?;
        let skill = native(client.skill(policy, &state))?;
        save_text(&local.directory.join("SKILL.md"), &skill)?;
        output["skill"] = serde_json::json!(skill);
    }
    if json {
        stdout.push_str(&format!("{output}\n"));
    } else {
        stdout.push_str(&format!(
            "Policy {}\n{name}: {}\nRequest: {}\n{}\n",
            policy.id,
            if replayed {
                format!("replayed ({})", result.status)
            } else {
                result.status.clone()
            },
            result.id,
            result.transaction_url
        ));
        if let Some(skill) = output["skill"].as_str() {
            stdout.push('\n');
            stdout.push_str(skill);
        }
    }
    if result.expired() {
        stderr.push_str("The original operation remains uncertain and cannot be broadcast again. For an explicitly additional fund/withdraw, set both a fresh ALLOWIT_REQUEST_ID and ALLOWIT_ADDITIONAL_OWNER_OPERATION=1; retain the old proof.\n");
    }
    if result.extra.get("decisionCode") == Some(&serde_json::json!("EXPIRED_UNEXECUTED")) {
        stderr.push_str("This request expired without execution. For a deliberate retry, set a new ALLOWIT_REQUEST_ID; keep the original journal.\n");
    }
    if audit_pending {
        if broadcast_withheld {
            stderr.push_str("Broadcast was withheld. Retry the identical execute command with this request ID to report and broadcast the saved proof; policy status only observes and reports.\n");
        }
        stderr.push_str("Native SQL audit is pending. Retain the original journal and request ID; retry the identical command or run allowit policy status.\n");
        return Ok(5);
    }
    Ok(if result.status == "failed" {
        20
    } else if result.status != "settled" {
        5
    } else if replayed {
        6
    } else {
        0
    })
}
