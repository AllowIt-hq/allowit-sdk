//! Native Stellar: owner signing, policy approval and installed wallet execution.
//! Backs `allowit wallet configure`, `allowit wallet approve`,
//! `allowit policy deploy --network stellar:testnet` and `exec`/`status` with
//! `ALLOWIT_STELLAR_BUNDLE`.
pub mod approval;
pub mod owner;
pub(crate) mod run;
