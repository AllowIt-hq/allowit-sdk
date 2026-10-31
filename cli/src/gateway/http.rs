//! HTTP tool syntax shared by the curl and http commands. No external HTTP tool runs here.
//! Normalizes curl, wget and HTTPie arguments into one [`HttpRequest`] that the
//! backend sends.
use super::args::{Invocation, body_file, public_url};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<Header>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

fn header(raw: &str) -> Result<Header> {
    let (name, value) = raw
        .split_once(':')
        .ok_or_else(|| Error::usage("--header requires Name: value"))?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        || value.contains(['\r', '\n'])
    {
        return Err(Error::usage("invalid HTTP header"));
    }
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization"
            | "cookie"
            | "proxy-authorization"
            | "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "x-payment"
    ) || lower.starts_with("payment-")
    {
        return Err(Error::usage(
            "backend connections own authorization and payment headers",
        ));
    }
    Ok(Header {
        name: name.into(),
        value: value.into(),
    })
}

/// Normalize HTTP tool syntax. No external HTTP tool or merchant transport runs here.
pub(crate) fn http_request(inv: &mut Invocation) -> Result<HttpRequest> {
    let mut method = None;
    let mut headers = vec![];
    let mut body = None;
    let mut target = None;
    let mut fields = BTreeMap::new();
    let mut i = 0;
    let mut literal = false;
    let mut content_type = None;
    let mut forms: Vec<(String, String)> = vec![];
    while i < inv.args.len() {
        let a = &inv.args[i];
        if a == "--" && !literal {
            literal = true;
            i += 1;
            continue;
        }
        let (name, inline) = a
            .split_once('=')
            .filter(|(n, _)| n.starts_with('-'))
            .map(|(n, v)| (n, Some(v.to_owned())))
            .unwrap_or((a.as_str(), None));
        if !literal
            && matches!(
                name,
                "-X" | "--request"
                    | "--method"
                    | "-H"
                    | "--header"
                    | "-d"
                    | "--data"
                    | "--data-raw"
                    | "--data-binary"
                    | "--body"
                    | "--body-data"
                    | "--post-data"
                    | "--body-file"
                    | "--post-file"
                    | "--content-type"
                    | "--form"
                    | "--form-file"
                    | "-o"
                    | "--output"
                    | "-O"
            )
        {
            let v = match inline {
                Some(v) => v,
                None => {
                    i += 1;
                    inv.args
                        .get(i)
                        .cloned()
                        .ok_or_else(|| Error::usage(format!("{name} requires a value")))?
                }
            };
            match name {
                "-X" | "--request" | "--method" => method = Some(v.to_ascii_uppercase()),
                "-H" | "--header" => headers.push(header(&v)?),
                "-o" | "--output" | "-O" => inv.common.output = Some(v.into()),
                "--content-type" => {
                    header(&format!("Content-Type: {v}"))?;
                    content_type = Some(v);
                }
                "--form" | "--form-file" => {
                    let (k, value) = v
                        .split_once('=')
                        .ok_or_else(|| Error::usage("form fields require NAME=VALUE"))?;
                    if k.is_empty() || k.contains(['\r', '\n', '"']) {
                        return Err(Error::usage("invalid form field name"));
                    }
                    forms.push((
                        k.into(),
                        if name == "--form-file" {
                            body_file(value)?
                        } else {
                            value.into()
                        },
                    ));
                }
                _ => {
                    let v = if matches!(name, "--body-file" | "--post-file") {
                        body_file(&v)?
                    } else if name != "--data-raw" && v.starts_with('@') {
                        body_file(&v[1..])?
                    } else {
                        v
                    };
                    if body.replace(v).is_some() {
                        return Err(Error::usage("give one request body"));
                    }
                }
            }
        } else if !literal
            && matches!(
                name,
                "-s" | "--silent" | "-S" | "--show-error" | "-f" | "--fail" | "--yes" | "-y"
            )
        {
            // These local display/confirmation flags never select a payment protocol.
        } else if target.is_none()
            && ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"].contains(&a.as_str())
            && inv.group == "http"
        {
            method = Some(a.clone());
        } else if a.starts_with("https://") || a.starts_with("http://") {
            if target.replace(a.clone()).is_some() {
                return Err(Error::usage("one URL is allowed per operation"));
            }
        } else if inv.group == "http" && a.contains('=') && !a.starts_with('-') {
            let (k, v) = a.split_once('=').unwrap();
            fields.insert(k.to_owned(), Value::String(v.to_owned()));
        } else {
            return Err(Error::usage(format!(
                "unsupported {} argument {}",
                inv.group,
                crate::error::quoted(a)
            )));
        }
        i += 1;
    }
    if !fields.is_empty() {
        if body.is_some() || !forms.is_empty() {
            return Err(Error::usage(
                "HTTPie fields and other bodies cannot be combined",
            ));
        }
        body = Some(serde_json::to_string(&fields).unwrap());
        content_type = Some("application/json".into());
    }
    if !forms.is_empty() {
        if body.is_some() {
            return Err(Error::usage("form fields and --body cannot be combined"));
        }
        let boundary = format!("AllowitBoundary{}", std::process::id());
        let mut encoded = String::new();
        for (k, v) in forms {
            encoded += &format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
            );
        }
        encoded += &format!("--{boundary}--\r\n");
        body = Some(encoded);
        content_type = Some(format!("multipart/form-data; boundary={boundary}"));
    }
    if let Some(mime) = content_type {
        headers.retain(|h| !h.name.eq_ignore_ascii_case("content-type"));
        headers.push(Header {
            name: "Content-Type".into(),
            value: mime,
        });
    } else if body.is_some()
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(Header {
            name: "Content-Type".into(),
            value: if inv.group == "curl" {
                "application/x-www-form-urlencoded"
            } else {
                "text/plain"
            }
            .into(),
        });
    }
    let method = method.unwrap_or_else(|| {
        if body.is_some() {
            "POST".into()
        } else {
            "GET".into()
        }
    });
    if !["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"].contains(&method.as_str()) {
        return Err(Error::usage("unsupported HTTP request method"));
    }
    let url = target.ok_or_else(|| {
        Error::usage(format!(
            "usage: allowit {} URL [request options]",
            inv.group
        ))
    })?;
    public_url(&url)?;
    if body.as_ref().is_some_and(|s| s.len() > 32 << 10) {
        return Err(Error::usage("request body exceeds 32768 bytes"));
    }
    Ok(HttpRequest {
        method,
        url,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::args::parse;
    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn http_adapters_do_not_accept_payment_headers() {
        let mut i = parse(&args(&[
            "curl",
            "https://example.com",
            "-H",
            "Payment-Signature: forged",
        ]))
        .unwrap();
        assert!(http_request(&mut i).is_err());
    }
    #[test]
    fn curl_body_is_bound_to_backend_request() {
        let mut i = parse(&args(&[
            "curl",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            "{\"q\":\"x\"}",
            "https://example.com",
        ]))
        .unwrap();
        let r = http_request(&mut i).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.body.as_deref(), Some("{\"q\":\"x\"}"));
    }
}
