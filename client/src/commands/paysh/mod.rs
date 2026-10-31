//! `allowit paysh ...`: retained Solana PaySH operations from a legacy policy,
//! through the pinned Solana vendor crate. Subcommands: `services`, `call`,
//! `status`. Reconciles existing operations only; holds no signing key.
//! `call` and `status` use `ALLOWIT_PAYSH_TOKEN`. Refuses `--network`.
//!
//! ```text
//! allowit paysh services --json
//! ALLOWIT_PAYSH_TOKEN=CAPABILITY \
//!   allowit paysh call POLICY OPERATION_ID SERVICE_ID '{"query":"..."}'
//! allowit paysh status POLICY OPERATION_ID --json
//! ```
use crate::{
    error::{Error, Result},
    routing,
};

pub(crate) fn run(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if routing::selected().is_some() {
        return Err(Error::unsupported(
            "Legacy PaySH reconciliation uses its retained policy network; explicit network override is not supported",
        ));
    }
    crate::vendors::paysh::run(args, stdout, stderr)
}
