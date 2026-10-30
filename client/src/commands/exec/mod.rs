//! `allowit exec POLICY [request flags]`: submits the request for execution.
//! Posts once to `/api/harness/OWNER/POLICY/transactions`, then polls status for
//! up to `--wait`. A retry must reuse the same `--request-id`. Tempo and
//! installed Stellar wallets (`ALLOWIT_STELLAR_BUNDLE`) use their vendor flows;
//! Stellar takes a `--request-file`.
//!
//! ```text
//! allowit eval POLICY --action transfer --amount 5 --merchant MERCHANT \
//!   --request-id eval-0002
//! allowit exec POLICY --action transfer --amount 5 --merchant MERCHANT \
//!   --request-id exec-0002 --wait 120s
//! # exit 5 (uncertain): rerun the identical command, or check it:
//! allowit status POLICY exec-0002 --wait 60s
//! ```
use crate::{action, error::Result};

pub(crate) fn run(argv: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    action::submit("exec", argv, stdout, stderr)
}
