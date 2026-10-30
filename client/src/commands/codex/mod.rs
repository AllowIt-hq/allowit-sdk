//! `allowit codex -- [native arguments]`: launches Codex with the Allowit MCP server.
use crate::{agents, error::Result, gateway::args::Invocation};

pub(crate) fn run(inv: &Invocation) -> Result<i32> {
    agents::run("codex", &inv.args, &inv.common)
}
