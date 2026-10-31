//! `allowit evaluate POLICY.rs CONTEXT.json [oracle|contract]`: prints the
//! decision for one request under the `oracle` (default) or `contract`
//! profile. `-` reads the source or the context from stdin. Local only.
//!
//! ```text
//! allowit evaluate docs/examples/approval.rs docs/examples/context.json contract
//! ```
use super::compiler::{self, failure};
use crate::error::Result;
use serde_json::{Value, json};

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    let source = compiler::source(args)?;
    let context = compiler::read(
        args.get(1)
            .ok_or_else(|| failure("Supply a context JSON file."))?,
    )?;
    let context: Value = serde_json::from_str(&context).map_err(|e| failure(e.to_string()))?;
    let profile = args.get(2).map(String::as_str).unwrap_or("oracle");
    compiler::process(
        json!({"operation":"evaluate","source":source,"context":context,"profile":profile}),
        stdout,
    )
}
