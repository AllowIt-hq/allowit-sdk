#[cfg(feature = "compiler")]
use crate::{Context, Profile};
#[cfg(feature = "compiler")]
use alloc::format;
use alloc::string::{String, ToString};
use serde_json::{Value, json};

fn failure(code: &str, message: &str) -> Value {
    json!({"ok":false,"error":{"code":code,"message":message}})
}

/// JSON wire interface shared by the CLI, browser/Go WebAssembly host and native engine.
pub fn process_json(input: &str) -> String {
    if input.len() > 262144 {
        return failure("REQUEST_TOO_LARGE", "The request exceeds 256 KiB.").to_string();
    }
    match serde_json::from_str::<Value>(input) {
        Ok(request) => process_value(request).to_string(),
        Err(_) => failure("INVALID_JSON", "The request is not valid JSON.").to_string(),
    }
}
pub fn process_value(request: Value) -> Value {
    let Some(operation) = request.get("operation").and_then(Value::as_str) else {
        return failure("INVALID_REQUEST", "Specify an operation.");
    };
    let allowed: &[&str] = match operation {
        "registry" => &["operation"],
        "evaluate" => &["operation", "source", "profile", "context"],
        _ => &["operation", "source"],
    };
    if request
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !allowed.contains(&key.as_str())))
    {
        return failure(
            "INVALID_REQUEST",
            "The request contains unsupported fields. Evaluation requires source, not supplied executable IR.",
        );
    }
    if operation == "registry" {
        return json!({"ok":true,"language":crate::LANGUAGE,"registry_version":crate::REGISTRY_VERSION,"functions":crate::registry()});
    }
    #[cfg(feature = "compiler")]
    {
        if !["compile", "check", "workflow", "evaluate"].contains(&operation) {
            return failure(
                "INVALID_OPERATION",
                "Use compile, check, workflow, evaluate or registry.",
            );
        }
        let Some(source) = request.get("source").and_then(Value::as_str) else {
            return failure("INVALID_REQUEST", "Supply Rust policy source.");
        };
        let policy = match crate::compile(source) {
            Ok(policy) => policy,
            Err(error) => return json!({"ok":false,"error":error}),
        };
        if operation != "evaluate" {
            return json!({"ok":true,"policy":policy});
        }
        let profile: Profile = match request.get("profile").cloned() {
            Some(v) => match serde_json::from_value(v) {
                Ok(p) => p,
                Err(_) => return failure("INVALID_PROFILE", "Choose oracle or contract."),
            },
            None => return failure("INVALID_PROFILE", "Choose oracle or contract."),
        };
        let context: Context = match request.get("context").cloned() {
            Some(v) => match serde_json::from_value(v) {
                Ok(ctx) => ctx,
                Err(e) => {
                    return failure(
                        "INVALID_CONTEXT",
                        &format!("Invalid evaluation context: {e}"),
                    );
                }
            },
            None => return failure("INVALID_CONTEXT", "Supply an evaluation context."),
        };
        json!({"ok":true,"decision":crate::evaluate(&policy,profile,&context),"source_hash":policy.source_hash,"ir_hash":policy.ir_hash})
    }
    #[cfg(not(feature = "compiler"))]
    {
        let _ = request;
        failure(
            "COMPILER_UNAVAILABLE",
            "The contract build evaluates authenticated IR; it does not parse source.",
        )
    }
}
