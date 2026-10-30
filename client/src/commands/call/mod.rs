//! `allowit call SERVICE_OR_URL [--input JSON]`: one backend-paid service call.
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
