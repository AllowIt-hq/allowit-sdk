//! `allowit eval POLICY [request flags]`: checks permission and spends nothing.
use crate::{action, error::Result};

pub(crate) fn run(argv: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    action::submit("eval", argv, stdout, stderr)
}
