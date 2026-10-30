//! `allowit tempo ...`: Tempo execution and paid calls through the pinned Tempo
//! crate. Subcommands: `import`, `execute`, `pay`, `call`, `resume`, `submit`,
//! `status`. Reads the executor bundle (`ALLOWIT_TEMPO_BUNDLE`, default
//! `.allowit-tempo/executor.json`); `pay` signs with
//! `ALLOWIT_TEMPO_EXECUTOR_KEY_FILE` and sends through Tempo RPC. With
//! `--network`, the bundle network must match. Run `allowit tempo help`.
//!
//! ```text
//! allowit tempo import executor.json
//! ALLOWIT_REQUEST_ID=pay-0001 allowit tempo execute RECIPIENT 1.5   # plan only
//! ALLOWIT_REQUEST_ID=pay-0001 allowit tempo pay RECIPIENT 1.5 --json
//! allowit tempo status pay-0001
//! allowit tempo call https://api.example/v1/data --id call-0001 --max-amount 0.050000
//! ```
use crate::error::Result;

pub(crate) fn run(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    crate::vendors::tempo::command(args, stdout, stderr)
}
