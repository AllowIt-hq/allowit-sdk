//! `allowit curl URL [curl options]`, with `wget` and `fetch` as aliases.
//! The backend sends the request; no local HTTP tool runs.
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
