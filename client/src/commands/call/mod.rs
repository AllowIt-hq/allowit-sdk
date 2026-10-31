//! `allowit call SERVICE_OR_URL [--input JSON]`: one backend-paid service call.
//! The target is a catalog service ID or an HTTP(S) URL. Posts to
//! `/api/gateway/quote`, checks the quote, then posts to `/api/gateway/call`.
//! Uses `ALLOWIT_ACCOUNT_TOKEN`, or `ALLOWIT_TOKEN` with `--policy version@addr`.
//! `--max-payment` caps the debit; `--request-id` sets the operation ID.
//!
//! ```text
//! export ALLOWIT_URL=https://allowit.example ALLOWIT_ACCOUNT_TOKEN=TOKEN
//! allowit skills search weather
//! allowit call SERVICE_ID --input '{"city":"Paris"}' --max-payment 0.05 \
//!   --request-id weather-0001 --json
//! allowit call status weather-0001
//! ```
mod status;

use crate::{
    error::Result,
    gateway::{
        args::{self, Invocation},
        operation::purchase,
        target::call_target,
    },
};
use serde_json::json;

pub(crate) fn run(inv: &Invocation, stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if inv.args.first().is_some_and(|s| s == "status") {
        return status::run(inv, stdout, stderr);
    }
    let (service, request, input) = call_target(inv)?;
    let mut q = args::context(&inv.common);
    if let Some(s) = service {
        q["serviceId"] = json!(s);
    }
    if let Some(r) = request {
        q["request"] = json!(r);
    }
    if let Some(i) = input {
        q["input"] = i;
    }
    purchase(q, &inv.common, stdout, stderr)
}
