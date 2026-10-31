//! `allowit registry`: prints every supported policy function with its title,
//! signature and description. Local only.
//!
//! ```text
//! allowit registry
//! ```
use super::compiler;
use crate::error::Result;
use serde_json::json;

pub(crate) fn run(stdout: &mut String) -> Result<i32> {
    compiler::process(json!({"operation":"registry"}), stdout)
}
