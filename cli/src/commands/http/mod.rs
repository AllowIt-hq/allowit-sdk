//! `allowit http [METHOD] URL [NAME=VALUE ...]`: HTTPie-style request syntax.
//! The backend sends the request; no local HTTP tool runs.
//!
//! `NAME=VALUE` fields become a JSON body. Also accepts the curl options of
//! `allowit curl`. Quotes and calls through `/api/gateway/quote` and
//! `/api/gateway/call`.
//!
//! ```text
//! allowit http POST https://api.example/v1/translate text=hello lang=fr \
//!   --max-payment 0.02 --request-id translate-0001 --json
//! allowit call status translate-0001
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
