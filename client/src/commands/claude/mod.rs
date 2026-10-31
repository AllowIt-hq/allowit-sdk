//! `allowit claude -- [native arguments]`: launches Claude Code with the Allowit
//! MCP server (`allowit mcp`). Requires a Claude subscription sign-in; API-key
//! environment and flags are removed or refused. The agent gets only the
//! selected authority: the account token, or `ALLOWIT_TOKEN` with `--policy`.
//!
//! ```text
//! export ALLOWIT_URL=https://allowit.example ALLOWIT_ACCOUNT_TOKEN=TOKEN
//! allowit whoami
//! allowit claude --sandbox --max-payment 1 -- -p "Find a weather API and call it"
//! ```
use crate::{agents, error::Result, gateway::args::Invocation};

pub(crate) fn run(inv: &Invocation) -> Result<i32> {
    agents::run("claude", &inv.args, &inv.common)
}
