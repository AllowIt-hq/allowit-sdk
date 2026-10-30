//! Vendor connections. Command code reaches rails and payment protocols only through these modules.
pub(crate) mod paysh;
pub(crate) mod solana;
pub(crate) mod stellar;
pub(crate) mod tempo;
