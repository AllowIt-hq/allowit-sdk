//! `allowit wallet configure --network stellar:testnet ...`: prints a Stellar
//! owner signing link. Inputs: `--owner`, `--agent` and `--engine` G addresses,
//! `--request-id`, optional `--fee-per-operation`. The owner account must be a
//! fresh funded native account. The link carries public data in its fragment;
//! the owner signs in the browser. See [`crate::vendors::stellar::owner`].
//!
//! ```text
//! allowit wallet generate --network stellar:testnet --output agent.key
//! allowit wallet show --network stellar:testnet agent.key   # G_AGENT
//! allowit wallet configure --network stellar:testnet --owner G_OWNER \
//!   --agent G_AGENT --engine G_ENGINE --request-id setup-0001
//! # the owner opens the printed link and signs
//! allowit wallet approve --network stellar:testnet --policy-file policy.json \
//!   --owner G_OWNER --request-id approve-0001
//! ```
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    crate::vendors::stellar::owner::run(args, stdout)
}
