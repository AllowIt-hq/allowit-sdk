//! `allowit wallet configure --network stellar:testnet ...`: prints a Stellar owner signing link.
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    crate::vendors::stellar::owner::run(args, stdout)
}
