//! `allowit json [REQUEST.json|-]`: runs any SDK wire-protocol request, for
//! example `edit_preference` or `compile_stellar_wallet`. With no file, or
//! with `-`, it reads the request from stdin. Local only.
//!
//! ```text
//! echo '{"operation":"registry"}' | allowit json
//! ```
use super::compiler::{self, failure};
use crate::error::Result;
use serde_json::Value;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    let input = compiler::read(args.first().map(String::as_str).unwrap_or("-"))?;
    let request: Value = serde_json::from_str(&input).map_err(|e| failure(e.to_string()))?;
    compiler::process(request, stdout)
}
