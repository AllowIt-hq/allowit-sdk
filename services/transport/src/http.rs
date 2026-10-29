//! Single-attempt, bounded HTTP transport without ambient authorization loading.
use crate::rpc::{Error, ErrorKind, Options, Result};
use reqwest::{blocking::Client, redirect::Policy};
use std::{io::Read, time::Instant};

#[derive(Clone, Copy)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}
impl Method {
    fn wire(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
            Self::Patch => reqwest::Method::PATCH,
            Self::Delete => reqwest::Method::DELETE,
            Self::Head => reqwest::Method::HEAD,
            Self::Options => reqwest::Method::OPTIONS,
        }
    }
}
pub struct Request<'a> {
    pub method: Method,
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: Option<&'a [u8]>,
}
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}
impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("header_count", &self.headers.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}
impl Response {
    pub fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> {
        self.headers
            .iter()
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_slice())
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| std::str::from_utf8(value).ok())
    }
}
#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    options: Options,
}
impl HttpClient {
    pub fn new(options: Options) -> Result<Self> {
        Self::build(options, false)
    }
    pub fn without_connection_reuse(options: Options) -> Result<Self> {
        Self::build(options, true)
    }
    fn build(options: Options, one_connection: bool) -> Result<Self> {
        if options.timeout.is_zero()
            || options.max_response_bytes == 0
            || options.max_response_bytes.checked_add(1).is_none()
            || options.connect_timeout.is_some_and(|v| v.is_zero())
        {
            return Err(Error::new(ErrorKind::Configuration));
        }
        let mut builder = Client::builder()
            .retry(reqwest::retry::never())
            .redirect(Policy::none())
            .timeout(options.timeout)
            .min_tls_version(reqwest::tls::Version::TLS_1_2);
        if options.no_proxy {
            builder = builder.no_proxy();
        }
        if let Some(timeout) = options.connect_timeout {
            builder = builder.connect_timeout(timeout);
        }
        if one_connection {
            builder = builder.http1_only().pool_max_idle_per_host(0);
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| Error::new(ErrorKind::Configuration))?,
            options,
        })
    }
    pub fn send(&self, request: Request<'_>, deadline: Option<Instant>) -> Result<Response> {
        self.send_inner(request, deadline, true, false)
    }
    pub fn send_headers(
        &self,
        request: Request<'_>,
        deadline: Option<Instant>,
    ) -> Result<Response> {
        self.send_inner(request, deadline, false, false)
    }
    pub fn send_success_body(
        &self,
        request: Request<'_>,
        deadline: Option<Instant>,
    ) -> Result<Response> {
        self.send_inner(request, deadline, true, true)
    }
    fn send_inner(
        &self,
        request: Request<'_>,
        deadline: Option<Instant>,
        read_body: bool,
        success_only: bool,
    ) -> Result<Response> {
        let timeout = match deadline {
            Some(deadline) => self.options.timeout.min(
                deadline
                    .checked_duration_since(Instant::now())
                    .filter(|v| !v.is_zero())
                    .ok_or_else(|| Error::new(ErrorKind::Deadline))?,
            ),
            None => self.options.timeout,
        };
        let url =
            reqwest::Url::parse(request.url).map_err(|_| Error::new(ErrorKind::Configuration))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::new(ErrorKind::Configuration));
        }
        let mut outbound = self
            .client
            .request(request.method.wire(), url)
            .timeout(timeout);
        for (name, value) in request.headers {
            outbound = outbound.header(name, value);
        }
        if let Some(body) = request.body {
            outbound = outbound.body(body.to_vec());
        }
        let response = outbound.send().map_err(|e| {
            Error::new(if e.is_timeout() {
                ErrorKind::Timeout
            } else if e.is_connect() {
                ErrorKind::Connection
            } else {
                ErrorKind::Transport
            })
        })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().into(), value.as_bytes().to_vec()))
            .collect();
        let mut body = Vec::new();
        if read_body && (!success_only || (200..300).contains(&status)) {
            response
                .take((self.options.max_response_bytes + 1) as u64)
                .read_to_end(&mut body)
                .map_err(|_| Error::new(ErrorKind::Read))?;
            if body.len() > self.options.max_response_bytes {
                return Err(Error::new(ErrorKind::Size));
            }
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}
