//! Vendor connections. Command code reaches rails and payment protocols only through these modules.
//! `solana` and `stellar` are native in this crate; `tempo` and `paysh` wrap
//! pinned vendor crates.
pub(crate) mod paysh;
pub(crate) mod solana;
pub(crate) mod stellar;
pub(crate) mod tempo;
