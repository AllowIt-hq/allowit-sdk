//! `allowit wallet show --network CAIP2 PRIVATE_FILE`: prints the public
//! metadata of an existing signer file. Local only; prints no private material.
//! The output is the input that `allowit setup` accepts.
//!
//! ```text
//! allowit wallet generate --network stellar:testnet --output agent.key
//! allowit wallet show --network stellar:testnet agent.key > agent.json
//! allowit setup --input-file agent.json
//! ```
use crate::{
    error::{Error, Result},
    routing,
    wallet::inspect_wallet,
};
use std::path::Path;

pub(crate) fn run(path: &str, stdout: &mut String) -> Result<i32> {
    let value = inspect_wallet(
        &routing::selected().ok_or_else(|| Error::usage("wallet show requires --network"))?,
        Path::new(path),
    )?;
    *stdout += &crate::output::json(&serde_json::to_value(value).unwrap());
    Ok(0)
}
