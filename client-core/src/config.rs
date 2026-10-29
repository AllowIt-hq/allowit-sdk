use crate::error::{Error, Result};
use std::{env, net::IpAddr};
use url::Url;
pub fn env_value(name: &str) -> String {
    env::var(name).unwrap_or_default()
}
pub fn parse_origin(raw: &str) -> Result<String> {
    if raw.is_empty() {
        return Ok("https://app.allowit.xyz".into());
    }
    let invalid = || Error::config("ALLOWIT_URL must be an origin such as https://allowit.example");
    let (scheme, rest) = raw.split_once("://").ok_or_else(invalid)?;
    let scheme = scheme.to_ascii_lowercase();
    let authority = rest.split('/').next().unwrap_or_default();
    let u = Url::parse(raw).map_err(|_| invalid())?;
    if u.cannot_be_a_base() || u.host_str().is_none() || authority.is_empty() {
        return Err(invalid());
    }
    if authority.contains('@') {
        return Err(Error::config("ALLOWIT_URL must not contain credentials"));
    }
    let path = &rest[authority.len()..];
    if !path.is_empty() && path != "/" || raw.contains(['?', '#']) {
        return Err(Error::config(
            "ALLOWIT_URL must be an origin without a path, query or fragment",
        ));
    }
    let host = if let Some(h) = authority.strip_prefix('[') {
        h.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    match scheme.as_str() {
        "https" => {}
        "http" => {
            if !host.eq_ignore_ascii_case("localhost")
                && !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
            {
                return Err(Error::config(
                    "ALLOWIT_URL must use https (http is allowed only for localhost)",
                ));
            }
        }
        _ => return Err(Error::config("ALLOWIT_URL must use https")),
    }
    let mut authority = authority.to_lowercase();
    if authority.ends_with(if scheme == "https" { ":443" } else { ":80" }) {
        authority = if host.contains(':') {
            format!("[{}]", host.to_lowercase())
        } else {
            host.to_lowercase()
        };
    }
    Ok(format!("{scheme}://{authority}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_are_strict() {
        assert_eq!(parse_origin("").unwrap(), "https://app.allowit.xyz");
        assert_eq!(
            parse_origin("https://EXAMPLE.com:443/").unwrap(),
            "https://example.com"
        );
        assert_eq!(parse_origin("http://[::1]:80").unwrap(), "http://[::1]");
        for s in [
            "http://example.com",
            "http://127.1",
            "https://example.com/.",
            "https://example.com?",
            "https://user@example.com",
        ] {
            assert!(parse_origin(s).is_err(), "{s}");
        }
    }
}
