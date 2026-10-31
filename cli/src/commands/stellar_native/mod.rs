//! `allowit stellar-native POLICY.rs`: prints the generated Rust for a custom
//! Stellar policy wallet, or a clear refusal. `-` reads the source from stdin.
//! Local only.
//!
//! ```text
//! allowit stellar-native docs/examples/approval.rs
//! ```
use super::compiler::{self, failure};
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    let source = compiler::source(args)?;
    let policy =
        allowit_sdk::compile_stellar_policy(&source).map_err(|e| failure(e.to_string()))?;
    stdout.push_str(&policy.rust_source);
    Ok(0)
}
