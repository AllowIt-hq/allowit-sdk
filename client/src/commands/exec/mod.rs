//! `allowit exec POLICY [request flags]`: submits the request.
use crate::{action, error::Result};

pub(crate) fn run(argv: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    action::submit("exec", argv, stdout, stderr)
}
