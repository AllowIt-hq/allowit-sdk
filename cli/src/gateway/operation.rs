//! Quote, call and status rendering for backend operations. [`purchase`] posts
//! `/api/gateway/quote`, checks the quote binding and expiry, then posts
//! `/api/gateway/call` once; an uncertain result points to `allowit call status`.
use super::{
    Gateway,
    args::{self, Common},
};
use crate::{
    error::{Error, Result},
    output,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn operation_id(common: &Common, request: &Value) -> String {
    if let Some(id) = &common.operation_id {
        return id.clone();
    }
    // A fresh invocation is a fresh user intent. Recovery uses the printed ID.
    let mut h = Sha256::new();
    h.update(b"allowit-command-operation-v1\0");
    h.update(std::process::id().to_le_bytes());
    h.update(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    h.update(request.to_string());
    format!("allowit-{:x}", h.finalize())[..48].into()
}

fn recovery_context(common: &Common) -> String {
    let mut result = String::new();
    for (flag, value) in [
        ("--policy", &common.policy_selector),
        ("--account", &common.account),
        ("--rail", &common.rail),
        ("--rpc", &common.rpc_url),
    ] {
        if let Some(v) = value {
            result += &format!(" {flag} '{}'", v.replace('\'', "'\\''"));
        }
    }
    if common.sandbox {
        result += " --sandbox";
    }
    result
}

pub(crate) fn purchase(
    mut quote: Value,
    common: &Common,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let id = operation_id(common, &quote);
    quote["operationId"] = json!(id);
    let client = Gateway::new(common)?;
    let response = client.send("POST", "/api/gateway/quote", Some(&quote))?;
    if response.get("operationId").and_then(Value::as_str) != Some(&id)
        || response.get("state").and_then(Value::as_str) != Some("quoted")
    {
        return Err(Error::uncertain(
            "Allowit quote does not match the requested operation",
        ));
    }
    let quote_id = response
        .get("quoteId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| Error::uncertain("Allowit quote lacks a valid quoteId"))?;
    if response
        .get("requestDigest")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(Error::uncertain("Allowit quote lacks its request binding"));
    }
    let expires = response
        .get("expiresAt")
        .and_then(Value::as_str)
        .and_then(|s| {
            time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
        })
        .filter(|t| *t > time::OffsetDateTime::now_utc());
    if expires.is_none() {
        return Err(Error::uncertain(
            "Allowit quote is expired or lacks a valid expiration",
        ));
    }
    let mut call = args::context(common);
    call["operationId"] = json!(id);
    call["quoteId"] = json!(quote_id);
    if common.max_debit.is_none()
        && let Some(limit) = response.get("maxDebitMoney")
    {
        call["maxDebitMoney"] = limit.clone();
    }
    let recovery = format!("allowit call status {id}{}", recovery_context(common));
    *stderr += &format!("allowit: operation {id}; if uncertain, check with {recovery}\n");
    crate::flush_stderr(stderr);
    let value = client
        .send("POST", "/api/gateway/call", Some(&call))
        .map_err(|mut e| {
            if e.code == 5 {
                e.message += &format!("; check {recovery}; do not create a new operation ID");
            }
            e
        })?;
    if value.get("operationId").and_then(Value::as_str) != Some(&id) {
        return Err(Error::uncertain(format!(
            "Allowit response does not match operation {id}; check its status"
        )));
    }
    if response.get("payment").is_some_and(Value::is_object)
        && matches!(value["state"].as_str(), Some("settled" | "delivered"))
    {
        let paid = &response["payment"];
        let settlement = &value["settlement"];
        if settlement["finalized"] != true
            || settlement["transactionId"]
                .as_str()
                .is_none_or(str::is_empty)
            || settlement["receiptDigest"]
                .as_str()
                .is_none_or(str::is_empty)
            || settlement["rail"] != paid["rail"]
            || settlement["network"] != paid["network"]
        {
            return Err(Error::uncertain(format!(
                "operation {id} lacks its matching finalized payment receipt; inspect status before retrying"
            )));
        }
    }
    render_operation(&value, common, stdout, stderr)
}

pub(crate) fn render_operation(
    value: &Value,
    common: &Common,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::uncertain("Allowit operation lacks a known state"))?;
    let exit = match state {
        "delivered" => 0,
        "denied" => 20,
        "awaiting_input" => 11,
        "pending" | "settled" => 12,
        "failed" => 4,
        "unknown" => 5,
        _ => {
            return Err(Error::uncertain(
                "Allowit returned an unknown operation state",
            ));
        }
    };
    let delivered = if state == "delivered" {
        let result = value
            .get("result")
            .filter(|v| v.is_object())
            .ok_or_else(|| Error::uncertain("delivered operation lacks its resource result"))?;
        let status = result
            .get("status")
            .and_then(Value::as_u64)
            .filter(|n| (100..=599).contains(n))
            .ok_or_else(|| Error::uncertain("delivered result lacks a valid HTTP status"))?;
        let body = result
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::uncertain("delivered result lacks its response body"))?;
        Some((status, body))
    } else {
        None
    };
    if common.json {
        *stdout += &output::json(value);
        if delivered.is_some_and(|(status, _)| status >= 400) {
            return Ok(4);
        }
    } else if let Some((status, body)) = delivered {
        if let Some(path) = &common.output {
            std::fs::write(path, body)
                .map_err(|_| Error::config("cannot write the selected output file"))?;
        } else {
            stdout.push_str(body);
        }
        if status >= 400 {
            *stderr += &format!("allowit: delivered upstream HTTP {status}\n");
            return Ok(4);
        }
    } else {
        *stderr += &format!(
            "allowit: operation state {state}; settlement and resource delivery remain separate\n"
        );
    }
    Ok(exit)
}
