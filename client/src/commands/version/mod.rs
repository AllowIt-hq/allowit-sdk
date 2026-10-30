//! `allowit version` (or `--version`): prints the CLI version. Local only.
//!
//! ```text
//! allowit version
//! # prints: allowit X.Y.Z
//! ```
use crate::{VERSION, error::Result};

pub(crate) fn run(stdout: &mut String) -> Result<i32> {
    *stdout += &format!("allowit {VERSION}\n");
    Ok(0)
}
