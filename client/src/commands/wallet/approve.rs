//! `allowit wallet approve --network stellar:testnet ...`: prints a Stellar source approval link.
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    crate::vendors::stellar::approval::run(args, stdout)
}
