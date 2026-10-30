//! `allowit mcp`: local MCP transport for the same Allowit backend command boundary.
mod tools;

use crate::{
    error::{Error, Result},
    gateway::args::Invocation,
};
use serde_json::{Value, json};
use std::io::{BufRead, Read, Write};

pub(crate) fn run(inv: &Invocation, stderr: &mut String) -> Result<i32> {
    if !inv.args.is_empty() {
        return Err(Error::usage(
            "usage: allowit mcp [--policy version@addr] [--sandbox]",
        ));
    }
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = std::io::Read::by_ref(&mut reader)
            .take(65_537)
            .read_until(b'\n', &mut line)
            .map_err(|_| Error::config("cannot read MCP input"))?;
        if n == 0 {
            break;
        }
        if line.len() > 65_536 {
            return Err(Error::usage("MCP message exceeds 65536 bytes"));
        }
        let message: Value = serde_json::from_slice(&line)
            .map_err(|_| Error::usage("MCP input must be a JSON object"))?;
        if message.get("id").is_none() {
            continue;
        }
        let id = message["id"].clone();
        let result = tools::dispatch(&message, inv, stderr);
        let response = match result {
            Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
            Err(e) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":crate::output::one_line(&e.message)}})
            }
        };
        writeln!(writer, "{}", crate::redact(response.to_string()))
            .map_err(|_| Error::config("cannot write MCP response"))?;
        writer
            .flush()
            .map_err(|_| Error::config("cannot flush MCP response"))?;
    }
    Ok(0)
}
