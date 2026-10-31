//! Shared input, read and mutation helpers for account and service controls.
//! Blockchain agent keys stay local. Used by `whoami`, `setup` and `skills`:
//! reads send `GET`, mutations send one `POST` under an operation ID.
use super::{
    Gateway,
    args::{self, Invocation},
    operation::{operation_id, render_operation},
};
use crate::{
    error::{Error, Result},
    output,
};
use serde_json::{Value, json};
use std::path::Path;

/// Named control options. Each one becomes a public input field.
const OPTIONS: &[&str] = &["name", "network", "service", "query", "limit"];

/// Control arguments: positional words and one public JSON input object.
pub(crate) struct ControlInput {
    pub pos: Vec<String>,
    pub input: Value,
}

pub(crate) fn input(inv: &Invocation) -> Result<ControlInput> {
    let args = &inv.args;
    let mut pos = vec![];
    let mut value = json!({});
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            i += 1;
            continue;
        }
        if a == "--input-file" {
            i += 1;
            let path = args
                .get(i)
                .ok_or_else(|| Error::usage("--input-file requires a path"))?;
            value = crate::typed_request::read_request_file::<Value>(Path::new(path))?;
            if !value.is_object() {
                return Err(Error::usage("--input-file must contain a JSON object"));
            }
        } else if a == "--input" {
            i += 1;
            let raw = args
                .get(i)
                .ok_or_else(|| Error::usage("--input requires one JSON object"))?;
            if raw.len() > 32 << 10 {
                return Err(Error::usage("--input exceeds 32768 bytes"));
            }
            value = serde_json::from_str(raw)
                .map_err(|_| Error::usage("--input must be valid JSON"))?;
            if !value.is_object() {
                return Err(Error::usage("--input must be a JSON object"));
            }
        } else if a.starts_with("--") {
            let name = a.trim_start_matches('-');
            if !OPTIONS.contains(&name) {
                return Err(Error::usage(format!("unknown control option --{name}")));
            }
            i += 1;
            let v = args
                .get(i)
                .ok_or_else(|| Error::usage(format!("--{name} requires a value")))?;
            if v.len() > 2048 || v.chars().any(char::is_control) {
                return Err(Error::usage("control option exceeds its text bounds"));
            }
            value[name] = json!(v);
        } else {
            pos.push(a.clone());
        }
        i += 1;
    }
    if let Some(currency) = &inv.common.currency {
        value["currency"] = json!(currency);
    }
    Ok(ControlInput { pos, input: value })
}

/// Refuses account input outside an allowlist, so future key fields cannot
/// silently become transport data.
pub(crate) fn public_fields(input: &Value, allowed: &[&str]) -> Result<()> {
    let fields = input
        .as_object()
        .ok_or_else(|| Error::usage("Account controls require a public JSON object"))?;
    if fields.keys().any(|name| !allowed.contains(&name.as_str())) {
        return Err(Error::usage(
            "Account input contains a field outside its public schema. Private keys, seeds, shares and wallet files must remain local",
        ));
    }
    if fields.values().any(|value| {
        value.as_str().is_none_or(|text| {
            text.is_empty() || text.len() > 2048 || text.chars().any(char::is_control)
        })
    }) {
        return Err(Error::usage(
            "Account public fields must contain bounded text",
        ));
    }
    Ok(())
}

/// Account commands act with direct account authority only.
pub(crate) fn require_account_authority(inv: &Invocation) -> Result<()> {
    if inv.common.policy_selector.is_some() {
        return Err(Error::usage(
            "this owner/account command requires direct account authority, without --policy",
        ));
    }
    Ok(())
}

/// Reads one backend route and prints its JSON. `query` is the optional search text.
pub(crate) fn read(
    inv: &Invocation,
    route: &str,
    query: Option<&String>,
    stdout: &mut String,
) -> Result<i32> {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in [
        ("account", &inv.common.account),
        ("policySelector", &inv.common.policy_selector),
        ("rail", &inv.common.rail),
        ("rpcUrl", &inv.common.rpc_url),
    ] {
        if let Some(s) = v {
            q.append_pair(k, s);
        }
    }
    q.append_pair("sandbox", if inv.common.sandbox { "true" } else { "false" });
    if let Some(s) = query {
        q.append_pair("query", s);
    }
    let value = Gateway::new(&inv.common)?.send("GET", &format!("{route}?{}", q.finish()), None)?;
    *stdout += &output::json(&value);
    Ok(0)
}

/// Sends one control mutation under a fresh or explicit operation ID.
pub(crate) fn mutate(
    inv: &Invocation,
    route: &str,
    action: &str,
    input: Value,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let mut request = args::context(&inv.common);
    request["action"] = json!(action);
    request["input"] = input;
    let id = operation_id(&inv.common, &request);
    request["operationId"] = json!(id);
    *stderr += &format!(
        "allowit: {} operation {id}; do not repeat an uncertain mutation with a new operation ID\n",
        inv.group
    );
    crate::flush_stderr(stderr);
    let value = Gateway::new(&inv.common)?.send("POST", route, Some(&request))?;
    if value.get("operationId").and_then(Value::as_str) != Some(id.as_str()) {
        return Err(Error::uncertain(
            "Allowit control response has a different operationId",
        ));
    }
    if value.get("state").is_some() {
        return render_operation(&value, &inv.common, stdout, stderr);
    }
    *stdout += &output::json(&value);
    Ok(0)
}
