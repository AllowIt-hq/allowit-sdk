//! `allowit policy generate PROMPT`: compiles and saves a new local policy.
use super::*;

pub(super) fn run(local: &Local, pos: &[String], json: bool, stdout: &mut String) -> Result<i32> {
    let policy = native(Policy::generate(&local.network, &pos[0]))?;
    native(
        FileJournal::new(local.directory.join("journal"))
            .locked(|| local.save_policy(&policy).map_err(to_native)),
    )?;
    if json {
        stdout.push_str(&serde_json::to_string(&policy).unwrap());
        stdout.push('\n');
    } else {
        stdout.push_str(&format!(
            "Policy {}\nNetwork: {}\nDaily limit: {} test tokens\n\n{}\n\nSaved {}\n",
            policy.id,
            policy.network,
            policy.daily_limit,
            policy.rust,
            local.policy_file.display()
        ));
    }
    Ok(0)
}
