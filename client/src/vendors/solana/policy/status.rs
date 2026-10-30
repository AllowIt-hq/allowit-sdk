//! `allowit policy status`: observes the vault and recovers journaled operations.
//! Reads the chain; needs no signer.
//!
//! ```text
//! ALLOWIT_REQUEST_ID=pay-0001 allowit policy execute TOKEN_ACCOUNT 0.25   # exit 5
//! allowit policy status --json
//! ```
use super::*;

pub(super) fn run(
    session: &Session,
    json: bool,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let Session {
        policy,
        owner,
        client,
        audit,
        journal,
        lifecycle,
        ..
    } = *session;
    let records = native(journal.entries::<Record>())?;
    let mut operations = Vec::new();
    let mut audit_pending = false;
    for record in records {
        operations.push(match lifecycle.recover(&record.id,policy,owner){Ok(r)=>{
            if r.method=="execute" && audit.as_ref().is_some_and(|a|a.report(&r).is_err()) {audit_pending=true;}
            r.public()
        },Err(_)=>{if record.method=="execute" && audit.is_some(){audit_pending=true;} serde_json::json!({"id":record.id,"status":"uncertain","error":"Saved operation could not be verified against this configuration"})}});
    }
    let last = native(journal.read::<serde_json::Value>("last"))?;
    let operation = last
        .as_ref()
        .and_then(|last| operations.iter().find(|r| r["id"] == last["id"]));
    let state = native(client.state(policy, owner, true, None))?;
    if json {
        stdout.push_str(&format!("{}\n",serde_json::json!({"policyId":policy.id,"network":policy.network,"state":state,"operations":operations,"operation":operation})));
    } else {
        stdout.push_str(&format!("Policy {}\n", policy.id));
        if let Some(s) = state {
            stdout.push_str(&format!(
                "Vault {}\nTrusted authority: {}\nApproved: {}\nBalance: {}\nPer-action limit: {}\nSpent today counter: {}\n",
                s.binding.vault,
                s.binding.authority,
                s.approved,
                allowit_native::policy::decimal(native(allowit_native::native::number(
                    &s.balance
                ))?),
                allowit_native::policy::decimal(native(allowit_native::native::number(
                    &s.action_limit
                ))?),
                allowit_native::policy::decimal(native(allowit_native::native::number(
                    &s.spent
                ))?)
            ));
        } else {
            stdout.push_str("Not deployed\n");
        }
        if let Some(op) = operation {
            stdout.push_str(&format!(
                "Last operation: {}\n{}\n",
                op["status"].as_str().unwrap_or("uncertain"),
                op["transactionUrl"].as_str().unwrap_or("")
            ));
        }
    }
    if audit_pending {
        stderr.push_str("Native SQL audit is pending; retain this journal and run allowit policy status again.\n");
    }
    Ok(if audit_pending { 5 } else { 0 })
}
