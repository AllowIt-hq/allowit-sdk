//! `allowit help`: prints the local usage (`usage.txt`) followed by the backend
//! command groups. Local only; sends nothing. With no command, the same usage
//! goes to stderr with exit 2. `allowit COMMAND --help` prints per-command help
//! for the backend groups (`call`, `curl`, `setup` and the others in
//! `gateway::args::GROUPS`) and for `policy`. `show`, `eval`, `exec` and
//! `status` reject `--help`; this page covers them.
//!
//! ```text
//! allowit help
//! allowit policy help
//! allowit call --help
//! ```
use crate::{error::Result, gateway::help::HELP};

pub(crate) const USAGE: &str = include_str!("usage.txt");

pub(crate) fn run(stdout: &mut String) -> Result<i32> {
    stdout.push_str(USAGE);
    stdout.push_str(HELP);
    Ok(0)
}

/// No command: usage on stderr, exit 2.
pub(crate) fn missing(stderr: &mut String) -> Result<i32> {
    stderr.push_str(USAGE);
    Ok(2)
}
