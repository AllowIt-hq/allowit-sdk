//! `allowit call status OPERATION_ID`: reads an operation without creating another purchase.
use crate::{
    error::{Error, Result},
    gateway::{
        Gateway,
        args::{self, Invocation},
        operation::render_operation,
    },
};
use serde_json::json;

pub(crate) fn run(inv: &Invocation, stdout: &mut String, stderr: &mut String) -> Result<i32> {
    if inv.args.len() != 2 {
        return Err(Error::usage(
            "usage: allowit call status OPERATION_ID [--policy version@addr]",
        ));
    }
    if !crate::request::request_id_valid(&inv.args[1]) {
        return Err(Error::usage("status requires the original operation ID"));
    }
    let mut c = args::context(&inv.common);
    c["operationId"] = json!(inv.args[1]);
    let v = Gateway::new(&inv.common)?.send("POST", "/api/gateway/operations/status", Some(&c))?;
    if v["operationId"].as_str() != Some(inv.args[1].as_str()) {
        return Err(Error::uncertain(
            "status response does not match the requested operation",
        ));
    }
    render_operation(&v, &inv.common, stdout, stderr)
}
