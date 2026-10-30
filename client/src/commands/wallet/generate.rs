//! `allowit wallet generate --network CAIP2 --output PRIVATE_FILE`: writes a new
//! local signer file (mode 0600) and prints only its public metadata
//! (`network`, `address`, `publicKey`, `keyType`). Local only; refuses an
//! existing file.
//!
//! ```text
//! allowit wallet generate --network solana:devnet --output agent.key
//! allowit wallet show --network solana:devnet agent.key > agent.json
//! allowit setup --input-file agent.json
//! ```
use crate::{
    error::{Error, Result},
    routing,
    wallet::generate_wallet,
};
use std::path::Path;

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    if args.len() != 3 || args[1] != "--output" {
        return Err(Error::usage(
            "allowit wallet generate --network NETWORK --output PRIVATE_FILE",
        ));
    }
    let n = routing::selected()
        .ok_or_else(|| Error::usage("Wallet generation requires --network CAIP2"))?;
    let value = generate_wallet(&n, Path::new(&args[2]))?;
    *stdout += &crate::output::json(&serde_json::to_value(value).unwrap());
    Ok(0)
}
