//! `allowit tempo ...`: Tempo execution and paid-call commands.
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    crate::vendors::tempo::command(args, stdout, stderr)
}
