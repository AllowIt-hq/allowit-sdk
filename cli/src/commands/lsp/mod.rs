//! `allowit lsp`: runs the policy language server on stdin and stdout, with
//! diagnostics, hover, completion and `allowit/workflow`. The server writes
//! its messages directly; this command adds no output of its own.
//!
//! ```text
//! allowit lsp
//! ```
use super::compiler::failure;
use crate::error::Result;

pub(crate) fn run() -> Result<i32> {
    allowit_sdk::lsp::run_stdio().map_err(|e| failure(e.to_string()))?;
    Ok(0)
}
