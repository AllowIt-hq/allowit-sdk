//! Bounded JSON-RPC transport. Retry and authority decisions belong to the caller.
use reqwest::{blocking::Client, redirect::Policy};
use serde_json::{Value, json};
use std::{
    fmt,
    io::Read,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeValidation {
    LegacyResult,
    StrictIdentity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Configuration,
    Deadline,
    Timeout,
    Connection,
    Transport,
    Http,
    Read,
    Size,
    Json,
    Envelope,
    Refused,
    MissingResult,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub http_status: Option<u16>,
    pub rpc_code: Option<i64>,
    pub retry_after_seconds: Option<u64>,
    well_formed_refusal: bool,
}
impl Error {
    pub fn is_well_formed_refusal(&self) -> bool {
        self.well_formed_refusal
    }
    pub(crate) fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            http_status: None,
            rpc_code: None,
            retry_after_seconds: None,
            well_formed_refusal: false,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RPC request failed ({:?}).", self.kind)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy)]
pub struct Options {
    pub timeout: Duration,
    pub max_response_bytes: usize,
    pub no_proxy: bool,
    pub connect_timeout: Option<Duration>,
}
#[derive(Clone)]
pub struct RpcClient {
    client: Client,
    endpoint: reqwest::Url,
    options: Options,
    accept_any_success: bool,
}
pub use crate::service::rpc::Operation;

impl RpcClient {
    pub fn accept_success_statuses(mut self) -> Self {
        self.accept_any_success = true;
        self
    }
    pub fn for_endpoint(&self, endpoint: &str) -> Result<Self> {
        let url =
            reqwest::Url::parse(endpoint).map_err(|_| Error::new(ErrorKind::Configuration))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::new(ErrorKind::Configuration));
        }
        Ok(Self {
            client: self.client.clone(),
            endpoint: url,
            options: self.options,
            accept_any_success: self.accept_any_success,
        })
    }
    pub fn new(endpoint: &str, timeout: Duration, max_response_bytes: usize) -> Result<Self> {
        Self::with_options(
            endpoint,
            Options {
                timeout,
                max_response_bytes,
                no_proxy: false,
                connect_timeout: None,
            },
        )
    }
    pub fn with_options(endpoint: &str, options: Options) -> Result<Self> {
        let url =
            reqwest::Url::parse(endpoint).map_err(|_| Error::new(ErrorKind::Configuration))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || options.timeout.is_zero()
            || options.max_response_bytes == 0
            || options.max_response_bytes.checked_add(1).is_none()
            || options.connect_timeout.is_some_and(|v| v.is_zero())
        {
            return Err(Error::new(ErrorKind::Configuration));
        }
        let mut builder = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .timeout(options.timeout)
            .min_tls_version(reqwest::tls::Version::TLS_1_2);
        if options.no_proxy {
            builder = builder.no_proxy();
        }
        if let Some(timeout) = options.connect_timeout {
            builder = builder.connect_timeout(timeout);
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| Error::new(ErrorKind::Configuration))?,
            endpoint: url,
            options,
            accept_any_success: false,
        })
    }
    pub fn call(&self, method: &str, params: &Value) -> Result<Value> {
        self.call_with_id(method, params, 1, EnvelopeValidation::LegacyResult, None)
    }
    pub fn call_before(
        &self,
        method: &str,
        params: &Value,
        deadline: Option<Instant>,
    ) -> Result<Value> {
        self.call_with_id(
            method,
            params,
            1,
            EnvelopeValidation::LegacyResult,
            deadline,
        )
    }
    pub fn call_operation(
        &self,
        operation: &impl Operation,
        id: u64,
        validation: EnvelopeValidation,
        deadline: Option<Instant>,
    ) -> Result<Value> {
        self.call_with_id(
            operation.method(),
            &operation.params(),
            id,
            validation,
            deadline,
        )
    }
    pub fn call_with_id(
        &self,
        method: &str,
        params: &Value,
        id: u64,
        validation: EnvelopeValidation,
        deadline: Option<Instant>,
    ) -> Result<Value> {
        let timeout = match deadline {
            Some(deadline) => self.options.timeout.min(
                deadline
                    .checked_duration_since(Instant::now())
                    .filter(|v| !v.is_zero())
                    .ok_or_else(|| Error::new(ErrorKind::Deadline))?,
            ),
            None => self.options.timeout,
        };
        let response = self
            .client
            .post(self.endpoint.clone())
            .timeout(timeout)
            .json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send()
            .map_err(|e| {
                Error::new(if e.is_timeout() {
                    ErrorKind::Timeout
                } else if e.is_connect() {
                    ErrorKind::Connection
                } else {
                    ErrorKind::Transport
                })
            })?;
        let status = response.status().as_u16();
        let retry_after_seconds = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        let contextual = |kind, rpc_code| Error {
            kind,
            http_status: Some(status),
            rpc_code,
            retry_after_seconds,
            well_formed_refusal: false,
        };
        if if self.accept_any_success {
            !(200..300).contains(&status)
        } else {
            status != 200
        } {
            return Err(contextual(ErrorKind::Http, None));
        }
        let mut bytes = Vec::new();
        response
            .take((self.options.max_response_bytes + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| contextual(ErrorKind::Read, None))?;
        if bytes.len() > self.options.max_response_bytes {
            return Err(contextual(ErrorKind::Size, None));
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| contextual(ErrorKind::Json, None))?;
        if validation == EnvelopeValidation::StrictIdentity
            && (body["jsonrpc"] != "2.0" || body["id"] != json!(id))
        {
            return Err(contextual(ErrorKind::Envelope, None));
        }
        if body.get("error").is_some_and(|value| !value.is_null()) {
            let mut error = contextual(ErrorKind::Refused, body["error"]["code"].as_i64());
            error.well_formed_refusal = body["error"].is_object()
                && body["error"]["message"].is_string()
                && body.get("result").is_none();
            return Err(error);
        }
        body.get("result")
            .cloned()
            .ok_or_else(|| contextual(ErrorKind::MissingResult, None))
    }
}
