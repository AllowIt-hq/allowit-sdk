//! Top-level command dispatch. Each command has its own folder; each
//! subcommand has its own file. Vendor connections live under `vendors/`.
//!
//! Two command families share one binary. Local and policy commands (`show`,
//! `eval`, `exec`, `status`, `policy`, `wallet`, `tempo`, `paysh`, `help`,
//! `version`) and the policy compiler commands (`compile`, `check`,
//! `workflow`, `evaluate`, `stellar-native`, `registry`, `json`, `lsp`) are
//! matched here. Backend command groups (`call`, `curl`, `wget`,
//! `fetch`, `http`, `claude`, `codex`, `mcp`, `whoami`, `setup`, `skills`) go
//! through [`gateway::args`] and call `/api/gateway/*` routes. For every
//! command except the compiler commands, `--network CAIP2` is extracted first
//! and may appear anywhere before `--`. The compiler commands skip network
//! selection and print the SDK output without redaction or display escaping.
//!
//! ```text
//! allowit help
//! allowit --json show POLICY        # global flags may precede the command
//! allowit skills list --sandbox
//! ```
mod call;
mod check;
mod claude;
mod codex;
mod compile;
mod compiler;
mod curl;
mod eval;
mod evaluate;
mod exec;
mod help;
mod http;
mod json;
mod lsp;
mod mcp;
mod paysh;
mod policy;
mod registry;
mod setup;
mod show;
mod skills;
mod status;
mod stellar_native;
mod tempo;
mod version;
mod wallet;
mod whoami;
mod workflow;

use crate::{
    error::{self, Error, Result},
    gateway::{self, help::command_help},
};

/// The policy compiler commands. They run the SDK locally, select no
/// network and read no environment.
const COMPILER: &[&str] = &[
    "compile",
    "check",
    "workflow",
    "evaluate",
    "stellar-native",
    "registry",
    "json",
    "lsp",
];

/// True when the command (after any leading global flags) is a policy
/// compiler command.
pub(crate) fn is_compiler(args: &[String]) -> bool {
    args.first()
        .is_some_and(|name| COMPILER.contains(&name.as_str()))
}

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
        "compile" => compile::run(rest, stdout),
        "check" => check::run(rest, stdout),
        "workflow" => workflow::run(rest, stdout),
        "evaluate" => evaluate::run(rest, stdout),
        "stellar-native" => stellar_native::run(rest, stdout),
        "registry" => registry::run(stdout),
        "json" => json::run(rest, stdout),
        "lsp" => lsp::run(),
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
