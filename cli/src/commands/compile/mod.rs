//! `allowit compile POLICY.rs`: prints the compiled policy as JSON: validated IR,
//! `source_hash`, `ir_hash`, execution requirements, the owner-facing
//! `workflow` step list and the recommended Stellar engines. `-` reads the
//! source from stdin. Local only; sends nothing.
//!
//! ```text
//! allowit compile docs/examples/research.rs
//! ```
use super::compiler;
use crate::error::Result;
use serde_json::json;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    compiler::process(
        json!({"operation":"compile","source":compiler::source(args)?}),
        stdout,
    )
}
