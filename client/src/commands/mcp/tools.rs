//! MCP methods and the Allowit tools they expose: `initialize`, `ping`,
//! `tools/list` and `tools/call`. `allowit_call` requires an `operationId` and
//! runs the same quote-then-call purchase as `allowit call`. Each message is
//! one line on stdin; it is wrapped here for reading.
//!
//! ```text
//! {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"allowit_call",
//!   "arguments":{"target":"SERVICE_ID","input":{"q":"news"},"operationId":"news-0001"}}}
//! {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"allowit_status",
//!   "arguments":{"operationId":"news-0001"}}}
//! ```
use crate::{
    error::{Error, Result},
    gateway::{
        Gateway,
        args::{self, Invocation},
        operation::purchase,
        target::call_target,
    },
};
use serde_json::{Value, json};

pub(super) fn dispatch(message: &Value, inv: &Invocation, stderr: &mut String) -> Result<Value> {
    match message["method"].as_str().unwrap_or("") {
        "initialize" => Ok(
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"allowit","version":crate::VERSION}}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":[
            {"name":"allowit_call","description":"Call a service through the selected Allowit account or explicit bound policy. The backend owns payment and connections.","inputSchema":{"type":"object","properties":{"target":{"type":"string"},"input":{},"operationId":{"type":"string"}},"required":["target","operationId"],"additionalProperties":false}},
            {"name":"allowit_services","description":"List services from the authenticated Allowit backend catalog.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
            {"name":"allowit_status","description":"Read an existing operation without creating another purchase.","inputSchema":{"type":"object","properties":{"operationId":{"type":"string"}},"required":["operationId"],"additionalProperties":false}}
        ]})),
        "tools/call" => {
            let name = message["params"]["name"].as_str().unwrap_or("");
            let a = &message["params"]["arguments"];
            let mut common = inv.common.clone();
            common.json = true;
            let mut out = String::new();
            let result = match name {
                "allowit_services" => Gateway::new(&common)?.send(
                    "GET",
                    &format!("/api/gateway/services?{}", args::query_context(&common)),
                    None,
                )?,
                "allowit_status" => {
                    let id = a["operationId"]
                        .as_str()
                        .filter(|s| crate::request::request_id_valid(s))
                        .ok_or_else(|| Error::usage("operationId is required"))?;
                    let mut q = args::context(&common);
                    q["operationId"] = json!(id);
                    let value = Gateway::new(&common)?.send(
                        "POST",
                        "/api/gateway/operations/status",
                        Some(&q),
                    )?;
                    if value["operationId"].as_str() != Some(id) {
                        return Err(Error::uncertain(
                            "status response does not match the requested operation",
                        ));
                    }
                    value
                }
                "allowit_call" => {
                    let target = a["target"]
                        .as_str()
                        .ok_or_else(|| Error::usage("target is required"))?;
                    let id = a["operationId"]
                        .as_str()
                        .filter(|s| crate::request::request_id_valid(s))
                        .ok_or_else(|| Error::usage("operationId is required"))?;
                    common.operation_id = Some(id.into());
                    let mut argv = vec![target.into()];
                    if let Some(v) = a.get("input") {
                        argv.push(v.to_string());
                    }
                    let call = Invocation {
                        group: "call".into(),
                        args: argv,
                        common: common.clone(),
                        help: false,
                    };
                    let (service, request, input) = call_target(&call)?;
                    let mut q = args::context(&common);
                    if let Some(v) = service {
                        q["serviceId"] = json!(v);
                    }
                    if let Some(v) = request {
                        q["request"] = json!(v);
                    }
                    if let Some(v) = input {
                        q["input"] = v;
                    }
                    let exit = purchase(q, &common, &mut out, stderr)?;
                    let v = serde_json::from_str::<Value>(&out).map_err(|_| {
                        Error::uncertain("Allowit operation response was not available")
                    })?;
                    if exit != 0 {
                        return Ok(
                            json!({"isError":true,"content":[{"type":"text","text":v.to_string()}]}),
                        );
                    }
                    v
                }
                _ => return Err(Error::usage("unknown Allowit MCP tool")),
            };
            Ok(json!({"content":[{"type":"text","text":result.to_string()}]}))
        }
        _ => Err(Error::usage("unknown MCP method")),
    }
}
