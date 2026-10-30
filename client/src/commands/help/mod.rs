//! `allowit help`: local usage followed by the backend command groups.
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
