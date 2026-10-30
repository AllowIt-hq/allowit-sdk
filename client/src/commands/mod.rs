//! Top-level command dispatch. Each command has its own folder; each
//! subcommand has its own file. Vendor connections live under `vendors/`.
mod call;
mod claude;
mod codex;
mod curl;
mod eval;
mod exec;
mod help;
mod http;
mod mcp;
mod paysh;
mod policy;
mod setup;
mod show;
mod skills;
mod status;
mod tempo;
mod version;
mod wallet;
mod whoami;

use crate::{
    error::{self, Error, Result},
    gateway::{self, help::command_help},
};

pub(crate) fn dispatch(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if args.is_empty() {
        return help::missing(stderr);
    }
    if gateway::args::recognizes(args) {
        return backend(args, stdout, stderr);
    }
    let rest = &args[1..];
    match args[0].as_str() {
        "version" | "--version" => version::run(stdout),
        "help" | "-h" | "--help" => help::run(stdout),
        "wallet" => wallet::run(rest, stdout),
        "policy" => policy::run(rest, stdout, stderr),
        "paysh" => paysh::run(rest, stdout, stderr),
        "tempo" => tempo::run(rest, stdout, stderr),
        "show" => show::run(rest, stdout, stderr),
        "eval" => eval::run(rest, stdout, stderr),
        "exec" => exec::run(rest, stdout, stderr),
        "status" => status::run(rest, stdout, stderr),
        _ => Err(Error::usage(format!(
            "unknown command {} (show, eval, exec, status, policy)",
            error::quoted(&args[0])
        ))),
    }
}

/// Commands served through the Allowit backend. The backend owns vendor payment protocols.
fn backend(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    let mut inv = gateway::args::parse(args)?;
    if inv.help {
        stdout.push_str(&command_help(&inv.group));
        return Ok(0);
    }
    match inv.group.as_str() {
        "call" => call::run(&inv, stdout, stderr),
        "curl" | "wget" | "fetch" => curl::run(&mut inv, stdout, stderr),
        "http" => http::run(&mut inv, stdout, stderr),
        "claude" => claude::run(&inv),
        "codex" => codex::run(&inv),
        "mcp" => mcp::run(&inv, stderr),
        "whoami" => whoami::run(&inv, stdout),
        "setup" => setup::run(&inv, stdout, stderr),
        "skills" => skills::run(&inv, stdout),
        _ => Err(Error::usage("unknown Allowit control command")),
    }
}

#[cfg(test)]
mod tests {
    fn run(args: &[&str]) -> crate::error::Result<i32> {
        let args: Vec<String> = args.iter().map(|s| (*s).into()).collect();
        let mut stdout = String::new();
        let mut stderr = String::new();
        crate::execute(&args, &mut stdout, &mut stderr)
    }

    #[test]
    fn global_json_before_a_local_command_reaches_that_command() {
        // Before the fix, a leading global flag routed to the backend parser,
        // which refused every local command.
        let error = run(&["--json", "show"]).unwrap_err();
        assert_eq!(error.code, 2);
        assert_eq!(
            error.message,
            "usage: allowit show POLICY [--json] [--source]"
        );
        let error = run(&["--json", "status", "policy"]).unwrap_err();
        assert!(error.message.starts_with("usage: allowit status POLICY"));
    }

    #[test]
    fn removed_commands_are_unknown() {
        for command in [
            "subscriptions",
            "install",
            "catalog",
            "account",
            "send",
            "topup",
            "server",
            "serve",
            "gate",
        ] {
            let error = run(&[command]).unwrap_err();
            assert_eq!(error.code, 2, "{command}");
            assert!(error.message.starts_with("unknown command"), "{command}");
        }
    }

    #[test]
    fn removed_skills_actions_are_refused_before_transport() {
        for action in ["add", "remove", "rm", "update", "prune-merged", "provider"] {
            let error = run(&["skills", action]).unwrap_err();
            assert_eq!(error.message, "unknown skills action", "{action}");
        }
    }
}
