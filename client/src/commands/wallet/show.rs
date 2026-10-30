//! `allowit wallet show --network CAIP2 PRIVATE_FILE`: prints public wallet metadata.
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
