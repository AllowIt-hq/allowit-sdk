//! Call targets shared by the call command and the MCP call tool.
use super::{
    args::{Invocation, body_file, public_url},
    http::{Header, HttpRequest},
};
use crate::error::{Error, Result};
use serde_json::Value;

pub(crate) fn call_target(
    inv: &Invocation,
) -> Result<(Option<String>, Option<HttpRequest>, Option<Value>)> {
    let mut args: Vec<&String> = vec![];
    let mut explicit_input: Option<String> = None;
    let mut i = 0;
    while i < inv.args.len() {
        let arg = &inv.args[i];
        if arg == "--" {
            i += 1;
            continue;
        }
        if matches!(arg.as_str(), "--input" | "--input-file") {
            i += 1;
            let value = inv.args.get(i).ok_or_else(|| {
                Error::usage("--input requires JSON and --input-file requires a path")
            })?;
            let raw = if arg == "--input-file" {
                body_file(value)?
            } else {
                value.clone()
            };
            if explicit_input.replace(raw).is_some() {
                return Err(Error::usage("give JSON input only once"));
            }
        } else if let Some(value) = arg.strip_prefix("--input=") {
            if explicit_input.replace(value.into()).is_some() {
                return Err(Error::usage("give JSON input only once"));
            }
        } else if arg.starts_with('-') {
            return Err(Error::usage("unknown allowit call option"));
        } else {
            args.push(arg);
        }
        i += 1;
    }
    let target = args.first().ok_or_else(|| {
        Error::usage(
            "usage: allowit call SERVICE_OR_URL [--input JSON] [--policy version@addr] [--sandbox]",
        )
    })?;
    if args.len() > 2 {
        return Err(Error::usage(
            "allowit call accepts one target and one JSON input",
        ));
    }
    if explicit_input.is_some() && args.len() > 1 {
        return Err(Error::usage("give JSON input only once"));
    }
    let raw = explicit_input.as_ref().or_else(|| args.get(1).copied());
    let input = if let Some(raw) = raw {
        if raw.len() > 32 << 10 {
            return Err(Error::usage("input exceeds 32768 bytes"));
        }
        Some(serde_json::from_str(raw).map_err(|_| Error::usage("INPUT_JSON must be valid JSON"))?)
    } else {
        None
    };
    if target.starts_with("http://") || target.starts_with("https://") {
        public_url(target)?;
        let body = input.as_ref().map(Value::to_string);
        Ok((
            None,
            Some(HttpRequest {
                method: if body.is_some() {
                    "POST".into()
                } else {
                    "GET".into()
                },
                url: (*target).clone(),
                headers: if body.is_some() {
                    vec![Header {
                        name: "Content-Type".into(),
                        value: "application/json".into(),
                    }]
                } else {
                    vec![]
                },
                body,
            }),
            None,
        ))
    } else {
        if target.len() > 300
            || !target
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
        {
            return Err(Error::usage("service name contains unsupported characters"));
        }
        Ok((Some((*target).clone()), None, input))
    }
}
