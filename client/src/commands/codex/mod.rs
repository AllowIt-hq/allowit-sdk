//! `allowit codex -- [native arguments]`: launches Codex with the Allowit MCP
//! server (`allowit mcp`). Requires a ChatGPT sign-in; API-key environment and
//! provider overrides are removed or refused. The agent gets only the selected
//! authority: the account token, or `ALLOWIT_TOKEN` with `--policy`.
//!
//! ```text
//! export ALLOWIT_URL=https://allowit.example ALLOWIT_TOKEN=OWNER.POLICY.SECRET
//! allowit codex --policy 3@POLICY_ADDRESS -- "Buy one search result under 0.05 USDC"
//! ```
use crate::{agents, error::Result, gateway::args::Invocation};

pub(crate) fn run(inv: &Invocation) -> Result<i32> {
    agents::run("codex", &inv.args, &inv.common)
}
