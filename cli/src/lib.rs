//! The `allowit` command. Argument parsing, terminal I/O and vendor command
//! code live here; backend requests and service composition come from
//! `allowit-sdk-engine`.
pub use allowit_native as native;
pub use allowit_sdk::admin;
pub use allowit_sdk_engine::{VERSION, error, wallet};
pub(crate) use allowit_sdk_engine::{client, config, output, request, skill, typed_request};
pub use allowit_stellar_wallet as stellar;
mod action;
mod agents;
mod args;
mod commands;
mod gateway;
pub mod routing;
#[cfg(test)]
mod test_http;
mod typed_run;
mod vendors;
use crate::{config::env_value, error::Result};
use serde_json::Value;
use std::io::Write;
pub use vendors::solana::handoff;
pub use vendors::stellar::{approval as stellar_approval, owner as stellar_owner};
/// Runs the command with process-local environment and standard streams.
pub fn run(args: Vec<String>) -> i32 {
    let args = gateway::args::local_command_first(&args);
    let mut stdout = String::new();
    let mut stderr = String::new();
    let result = execute(&args, &mut stdout, &mut stderr);
    let code = match result {
        Ok(c) => c,
        Err(e) => {
            stderr += &format!("allowit: {e}\n");
            e.code
        }
    };
    if commands::is_compiler(&args) {
        // The policy compiler commands read no environment or credentials.
        // They print the SDK output unchanged, as the former standalone
        // compiler binary did.
        let _ = std::io::stdout().write_all(stdout.as_bytes());
        let _ = std::io::stderr().write_all(stderr.as_bytes());
        return code;
    }
    let stdout = safe_json_output(stdout);
    let _ = std::io::stdout().write_all(redact(stdout).as_bytes());
    let _ = std::io::stderr().write_all(redact(stderr).as_bytes());
    code
}
fn safe_json_output(stdout: String) -> String {
    if serde_json::from_str::<Value>(&stdout).is_ok() {
        // Escape display controls within native JSON without changing the
        // parsed policy prompt, identity, or skill content.
        stdout
            .chars()
            .map(|c| {
                if ('\u{7f}'..'\u{a0}').contains(&c)
                    || ('\u{202a}'..='\u{202e}').contains(&c)
                    || ('\u{2066}'..='\u{2069}').contains(&c)
                {
                    format!("\\u{:04x}", c as u32)
                } else {
                    c.to_string()
                }
            })
            .collect()
    } else {
        stdout
    }
}
/// Removes the harness token and the PaySH capability from any output.
fn redact(s: String) -> String {
    output::clean(
        output::clean(
            output::clean(s, &env_value("ALLOWIT_TOKEN")),
            &env_value(gateway::ACCOUNT_TOKEN_ENV),
        ),
        &env_value(vendors::paysh::TOKEN_ENV),
    )
}
/// Writes pending stderr now, before a request can leave the process.
pub(crate) fn flush_stderr(stderr: &mut String) {
    let _ = std::io::stderr().write_all(redact(std::mem::take(stderr)).as_bytes());
}
fn execute(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    // Network inference reads the command name, so order it first.
    let args = gateway::args::local_command_first(args);
    // The policy compiler commands are local. They take no network and do
    // not read the wallet environment that network selection reads.
    if commands::is_compiler(&args) {
        return commands::dispatch(&args, stdout, stderr);
    }
    let (args, network) = routing::extract(&args)?;
    routing::with_network(network, || commands::dispatch(&args, stdout, stderr))
}
#[cfg(test)]
mod display_tests {
    #[test]
    fn json_display_controls_are_escaped_independently_of_command_prefix() {
        let original = serde_json::json!({"prompt":"source\u{202e} text\u{0085}"});
        let encoded = super::safe_json_output(original.to_string());
        assert!(!encoded.contains('\u{202e}'));
        assert!(!encoded.contains('\u{0085}'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            original
        );
    }
}
