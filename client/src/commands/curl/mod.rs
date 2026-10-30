//! `allowit curl URL [curl options]`, with `wget` and `fetch` as aliases.
//! The backend sends the request; no local HTTP tool runs.
//!
//! Accepts `-X`, `-H`, `-d`/`--data` (`@file`), `--data-raw`, `--body-file`,
//! `--form`, `--content-type` and `-o FILE`. Authorization and payment headers
//! are refused. Quotes and calls through `/api/gateway/quote` and
//! `/api/gateway/call`, like `allowit call`.
//!
//! ```text
//! allowit curl https://api.example/v1/search -X POST \
//!   -H 'Content-Type: application/json' -d '{"q":"rust"}' \
//!   --max-payment 0.10 --request-id search-0001 -o result.json
//! allowit call status search-0001
//! ```
use crate::{
    error::Result,
    gateway::{
        args::{self, Invocation},
        http::http_request,
        operation::purchase,
    },
};
use serde_json::json;

pub(crate) fn run(inv: &mut Invocation, stdout: &mut String, stderr: &mut String) -> Result<i32> {
    let r = http_request(inv)?;
    let mut q = args::context(&inv.common);
    q["request"] = json!(r);
    purchase(q, &inv.common, stdout, stderr)
}
