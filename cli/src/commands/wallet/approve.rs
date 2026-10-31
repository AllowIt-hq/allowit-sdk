//! `allowit wallet approve --network stellar:testnet ...`: prints a Stellar
//! source approval link for an existing owner-configured policy. Inputs:
//! `--policy-file` (public policy JSON), `--owner` G address, `--request-id`.
//! Moves no funds. See [`crate::vendors::stellar::approval`].
//!
//! ```text
//! allowit wallet configure --network stellar:testnet --owner G_OWNER \
//!   --agent G_AGENT --engine G_ENGINE --request-id setup-0001
//! allowit wallet approve --network stellar:testnet --policy-file policy.json \
//!   --owner G_OWNER --request-id approve-0001
//! # the owner opens the printed link and approves
//! ALLOWIT_STELLAR_BUNDLE=bundle.json ALLOWIT_STELLAR_SIGNER=agent.key \
//!   allowit exec POLICY --request-file request.json --request-id exec-0001
//! ```
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    crate::vendors::stellar::approval::run(args, stdout)
}
