//! `allowit wallet SUBCOMMAND`: local agent keys and owner wallet handoffs.
//!
//! `generate` writes a new private signer file; `show` prints its public
//! metadata. Both are local only. `configure` and `approve` print Stellar owner
//! browser links; they hold no owner key and send nothing.
//!
//! ```text
//! allowit wallet generate --network stellar:testnet --output agent.key
//! allowit wallet show --network stellar:testnet agent.key > agent.json
//! allowit setup --input-file agent.json
//! allowit wallet configure --network stellar:testnet --owner G_OWNER \
//!   --agent G_AGENT --engine G_ENGINE --request-id setup-0001
//! ```
mod approve;
mod configure;
mod generate;
mod show;

use crate::error::{Error, Result};

pub(crate) fn run(args: &[String], stdout: &mut String) -> Result<i32> {
    match args.first().map(String::as_str) {
        Some("configure") => configure::run(&args[1..], stdout),
        Some("approve") => approve::run(&args[1..], stdout),
        Some("generate") => generate::run(args, stdout),
        Some("show") if args.len() == 2 => show::run(&args[1], stdout),
        _ => Err(Error::usage(
            "allowit wallet generate --network NETWORK --output PRIVATE_FILE | allowit wallet show PRIVATE_FILE",
        )),
    }
}
