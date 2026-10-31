//! Native Solana: policy lifecycle, owner browser handoff and SQL audit reporting.
//! Backs `allowit policy` on `solana:testnet` and `solana:devnet`.
pub(crate) mod audit;
pub mod handoff;
pub(crate) mod policy;
