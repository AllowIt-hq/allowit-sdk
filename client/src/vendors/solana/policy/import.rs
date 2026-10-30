//! `allowit policy import EXECUTOR_JSON`: imports an owner-issued executor bundle.
use super::*;

pub(super) fn run(local: &Local, pos: &[String], json: bool, stdout: &mut String) -> Result<i32> {
    let bundle: Bundle = read_optional(Path::new(&pos[0]))?
        .ok_or_else(|| Error::config("Cannot read executor bundle"))?;
    native(bundle.policy.validate())?;
    if let Some(audit) = &bundle.audit {
        audit.validate()?;
    }
    let c = &bundle.context;
    if bundle.version != 2 || c.policy_id != bundle.policy.id || c.network != bundle.policy.network
    {
        return Err(Error::config("Bundle network/owner mismatch"));
    }
    let imported = native(NativeClient::new(
        Config {
            network: c.network.clone(),
            mint: Some(c.mint),
            executor: Some(c.executor),
            authority: Some(c.authority),
            deployment: Some(c.deployment.clone()),
        },
        Arc::new(OfflineRpc),
    ))?;
    native(imported.public_binding(&bundle.policy, c.owner))?;
    let journal = FileJournal::new(local.directory.join("journal"));
    native(journal.locked(|| {
        if fs::symlink_metadata(&local.policy_file).is_ok() {return Err(allowit_native::error::Error::config("A policy already exists here. Choose a new ALLOWIT_POLICY_DIR; keep the previous policy and journal for recovery."));}
        if let Some(existing)=&local.context {
            if serde_json::to_vec(existing).unwrap()!=serde_json::to_vec(c).unwrap() {return Err(allowit_native::error::Error::config("A different public context already exists here; choose a new policy directory"));}
        } else {create_private(&local.directory.join("context.json"),c,"A different public context already exists here; choose a new policy directory").map_err(to_native)?;}
        if let Some(audit)=&bundle.audit {journal.write("audit",audit)?;}
        local.save_policy(&bundle.policy).map_err(to_native)
    }))?;
    if json {
        stdout.push_str(&format!(
            "{}\n",
            serde_json::json!({"policyId":bundle.policy.id,"owner":c.owner,"imported":true})
        ));
    } else {
        stdout.push_str(&format!("Imported policy {}. Configure only ALLOWIT_EXECUTOR_KEYPAIR on the executor device; never the owner key.\n",bundle.policy.id));
    }
    Ok(0)
}
