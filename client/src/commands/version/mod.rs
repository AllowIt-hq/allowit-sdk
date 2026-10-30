//! `allowit version`.
use crate::{VERSION, error::Result};

pub(crate) fn run(stdout: &mut String) -> Result<i32> {
    *stdout += &format!("allowit {VERSION}\n");
    Ok(0)
}
