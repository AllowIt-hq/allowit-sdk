//! `allowit paysh ...`: retained Solana PaySH operations.
use crate::{
    error::{Error, Result},
    routing,
};

pub(crate) fn run(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if routing::selected().is_some() {
        return Err(Error::unsupported(
            "Legacy PaySH reconciliation uses its retained policy network; explicit network override is not supported",
        ));
    }
    crate::vendors::paysh::run(args, stdout, stderr)
}
