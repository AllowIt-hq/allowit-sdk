//! `allowit claude -- [native arguments]`: launches Claude Code with the Allowit MCP server.
use crate::{agents, error::Result, gateway::args::Invocation};

pub(crate) fn run(inv: &Invocation) -> Result<i32> {
    agents::run("claude", &inv.args, &inv.common)
}
