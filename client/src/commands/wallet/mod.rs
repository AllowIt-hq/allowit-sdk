//! `allowit wallet SUBCOMMAND`: local agent keys and owner wallet handoffs.
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
