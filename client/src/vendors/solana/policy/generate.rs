//! `allowit policy generate PROMPT`: compiles and saves a new local policy.
//! Local only; never signs. Prints the generated Rust source. The prompt must
//! read "Spend up to N tokens per day" or "Spend up to N test tokens per day",
//! optionally followed by " with PaySH discovery"; other rules exit 20.
//!
//! ```text
//! allowit policy generate "Spend up to 5 test tokens per day"
//! ALLOWIT_OWNER=OWNER_PUBKEY ALLOWIT_REQUEST_ID=deploy-0001 allowit policy deploy 10
//! ```
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
