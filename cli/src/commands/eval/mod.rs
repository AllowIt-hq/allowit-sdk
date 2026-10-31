//! `allowit eval POLICY [request flags]`: asks the policy whether a request
//! would pass. Spends nothing. Posts to `/api/harness/OWNER/POLICY/judge` and
//! waits up to `--wait` (default 60s) for a pending evaluation. Use a separate
//! `--request-id` from the later `exec`. Tempo routes to the Tempo crate.
//!
//! ```text
//! allowit show POLICY
//! allowit eval POLICY --action transfer --rail solana --op transferUSDC \
//!   --addr SOLANA_ADDRESS --amount 2 --request-id eval-0001 --json
//! # exit 0: passed; 4: rejected; 11: awaiting owner input; 20: denied
//! allowit exec POLICY --action transfer --rail solana --op transferUSDC \
//!   --addr SOLANA_ADDRESS --amount 2 --request-id exec-0001
//! ```
use crate::{action, error::Result};

pub(crate) fn run(argv: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    action::submit("eval", argv, stdout, stderr)
}
