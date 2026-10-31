//! `allowit skills [list|search|show] [QUERY]`: reads the backend service catalog.
//! Every action reads `GET /api/gateway/services`; `QUERY` is sent as `query`.
//! `ls` and `endpoints` alias `list` and `show`.
//!
//! ```text
//! allowit skills list --sandbox
//! allowit skills search weather
//! allowit skills show SERVICE_ID
//! allowit call SERVICE_ID --input '{"city":"Paris"}' --sandbox
//! ```
mod list;
mod search;
mod show;

use crate::{
    error::{Error, Result},
    gateway::{args::Invocation, control},
};

pub(crate) const ROUTE: &str = "/api/gateway/services";

pub(crate) fn run(inv: &Invocation, stdout: &mut String) -> Result<i32> {
    let c = control::input(inv)?;
    let query = c.pos.get(1);
    match c.pos.first().map(String::as_str).unwrap_or("list") {
        "list" | "ls" => list::run(inv, query, stdout),
        "search" => search::run(inv, query, stdout),
        "show" | "endpoints" => show::run(inv, query, stdout),
        _ => Err(Error::usage("unknown skills action")),
    }
}
