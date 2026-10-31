//! Backend-owned service access. This client never constructs vendor payment proofs.
//! [`Gateway`] sends one request to an `/api/gateway/*` route under
//! `ALLOWIT_URL`, with `ALLOWIT_ACCOUNT_TOKEN`, or `ALLOWIT_TOKEN` when
//! `--policy` is given. Used by `call`, `curl`, `http`, `mcp`, `whoami`,
//! `setup` and `skills`.
pub(crate) mod args;
pub(crate) mod control;
pub(crate) mod help;
pub(crate) mod http;
pub(crate) mod operation;
pub(crate) mod target;

use crate::{
    VERSION,
    config::{env_value, parse_origin},
    error::{Error, Result},
    output,
};
use args::Common;
use reqwest::{blocking::Client, redirect::Policy, tls::Version};
use serde_json::Value;
use std::{io::Read, time::Duration};

pub(crate) const ACCOUNT_TOKEN_ENV: &str = "ALLOWIT_ACCOUNT_TOKEN";
const MAX_REPLY: usize = 2 << 20;

pub(crate) struct Gateway {
    origin: String,
    token: String,
    http: Client,
}
impl Gateway {
    pub(crate) fn new(common: &Common) -> Result<Self> {
        let name = if common.policy_selector.is_some() {
            "ALLOWIT_TOKEN"
        } else {
            ACCOUNT_TOKEN_ENV
        };
        let token = env_value(name);
        if token.starts_with("allowit-stellar-v1.") {
            return Err(Error::config(
                "Customer signing tokens are local-only and cannot be used as gateway bearer capabilities",
            ));
        }
        if token.is_empty() {
            return Err(Error::config(format!(
                "set {name} to the {} capability issued by the Allowit owner app",
                if common.policy_selector.is_some() {
                    "policy"
                } else {
                    "selected account"
                }
            )));
        }
        if token.len() < 16
            || token.len() > 4096
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(Error::config(format!(
                "{name} must contain the capability exactly as issued"
            )));
        }
        if common.policy_selector.is_some() {
            let p = token.split('.').collect::<Vec<_>>();
            if p.len() != 3 || p[0].is_empty() || p[1].is_empty() || p[2].len() < 16 {
                return Err(Error::config(
                    "ALLOWIT_TOKEN must be an owner.policy.secret capability",
                ));
            }
        }
        Self::connect(token)
    }
    fn connect(token: String) -> Result<Self> {
        let origin = parse_origin(&env_value("ALLOWIT_URL"))?;
        let mut builder = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(60))
            .min_tls_version(Version::TLS_1_2);
        let ca = env_value("ALLOWIT_CA_FILE");
        if !ca.is_empty() {
            let pem = std::fs::read(ca).map_err(|_| {
                Error::config("ALLOWIT_CA_FILE must name a readable PEM certificate file")
            })?;
            let certs = reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|_| Error::config("ALLOWIT_CA_FILE must contain PEM certificates"))?;
            if certs.is_empty() {
                return Err(Error::config(
                    "ALLOWIT_CA_FILE must contain PEM certificates",
                ));
            }
            for c in certs {
                builder = builder.add_root_certificate(c);
            }
        }
        Ok(Self {
            origin,
            token,
            http: builder
                .build()
                .map_err(|_| Error::config("cannot configure the Allowit HTTP client"))?,
        })
    }
    /// One request. A payment-bearing POST is never resent after uncertainty.
    pub(crate) fn send(&self, method: &str, route: &str, body: Option<&Value>) -> Result<Value> {
        if !route.starts_with("/api/gateway/") {
            return Err(Error::unsupported(
                "gateway route is outside the Allowit API",
            ));
        }
        let mut r = self
            .http
            .request(
                method
                    .parse()
                    .map_err(|_| Error::usage("invalid backend request method"))?,
                format!("{}{route}", self.origin),
            )
            .header("Accept", "application/json")
            .header("User-Agent", format!("allowit-cli/{VERSION}"));
        if !self.token.is_empty() {
            r = r.bearer_auth(&self.token);
        }
        if let Some(b) = body {
            let raw = serde_json::to_vec(b).map_err(|_| Error::usage("cannot encode request"))?;
            if raw.len() > 64 << 10 {
                return Err(Error::usage("Allowit request exceeds 65536 bytes"));
            }
            r = r.header("Content-Type", "application/json").body(raw);
        }
        let mut response = r.send().map_err(|_| {
            Error::uncertain("connection to Allowit failed; no automatic retry was made")
        })?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        Read::by_ref(&mut response)
            .take(MAX_REPLY as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                Error::uncertain("cannot read Allowit response; no automatic retry was made")
            })?;
        if bytes.len() > MAX_REPLY {
            return Err(Error::uncertain("Allowit response exceeds the 2 MB limit"));
        }
        if (300..400).contains(&status) {
            return Err(Error::uncertain(
                "Allowit returned a redirect; the capability was not forwarded",
            ));
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::uncertain("Allowit returned invalid JSON"))?;
        if !value.is_object() {
            return Err(Error::uncertain("Allowit response must be a JSON object"));
        }
        if !(200..300).contains(&status) {
            let code = value.get("code").and_then(Value::as_str).unwrap_or("");
            let message = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("request rejected");
            if matches!(code, "PAYMENT_UNCERTAIN" | "OPERATION_PENDING") || status >= 500 {
                return Err(Error::uncertain(format!(
                    "Allowit returned HTTP {status}; operation outcome requires status lookup"
                )));
            }
            if matches!(
                code,
                "POLICY_DENIED"
                    | "POLICY_NOT_ACTIVE"
                    | "DEPLOYMENT_BINDING_MISMATCH"
                    | "SANDBOX_BINDING_REQUIRED"
            ) {
                return Err(Error {
                    code: 20,
                    message: format!("{}: {}", output::one_line(code), output::one_line(message)),
                    status: Some(status),
                    config: false,
                });
            }
            return Err(Error::api(status, output::one_line(message)));
        }
        Ok(value)
    }
}
