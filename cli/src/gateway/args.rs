//! Public Allowit command input. Vendor payment protocols remain on the backend.
//! Parses the backend command group and its global flags (`--policy`,
//! `--sandbox`, `--account`, `--request-id`, `--rail` with `--rpc`,
//! `--max-payment`, `--currency`, `--json`) into an [`Invocation`].
pub(crate) use super::{Common, Money};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::io::Read;
use url::Url;

/// Top-level commands served through the Allowit backend.
pub(crate) const GROUPS: &[&str] = &[
    "call", "curl", "wget", "http", "fetch", "claude", "codex", "whoami", "mcp", "skills", "setup",
];

/// Global flags that may precede any command name.
const GLOBAL_VALUE_FLAGS: &[&str] = &[
    "--policy",
    "--account",
    "--request-id",
    "--operation-id",
    "--rail",
    "--rpc",
    "--max-payment",
    "--budget",
    "--currency",
    "--network",
];
const GLOBAL_SWITCHES: &[&str] = &["--sandbox", "--json"];

#[derive(Debug)]
pub(crate) struct Invocation {
    pub group: String,
    pub args: Vec<String>,
    pub common: Common,
    pub help: bool,
}

/// Index of the first argument that is neither a global flag nor a global flag value.
/// `None` means that every argument is a global flag.
pub(crate) fn command_index(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let (name, inline) = match arg.split_once('=') {
            Some((name, _)) => (name, true),
            None => (arg, false),
        };
        if GLOBAL_VALUE_FLAGS.contains(&name) {
            i += if inline { 1 } else { 2 };
        } else if GLOBAL_SWITCHES.contains(&name) {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

/// True only when the command name is a backend command group. Global flags
/// before a local command, such as `--json show`, stay with the local command.
pub(crate) fn recognizes(args: &[String]) -> bool {
    match command_index(args) {
        Some(i) => GROUPS.contains(&args[i].as_str()),
        // Only global flags: the backend parser reports the missing command.
        None => !args.is_empty(),
    }
}

/// Moves a local command name ahead of the global flags that precede it, so
/// `allowit --json show POLICY` reads as `allowit show --json POLICY` and
/// `allowit --json policy status` reads as `allowit policy status --json`.
/// Backend command groups parse their own leading global flags.
pub(crate) fn local_command_first(args: &[String]) -> Vec<String> {
    match command_index(args) {
        Some(i) if i > 0 && !GROUPS.contains(&args[i].as_str()) => {
            // Commands with subcommands read flags after the subcommand name.
            let words = if matches!(args[i].as_str(), "policy" | "wallet" | "tempo" | "paysh")
                && args.get(i + 1).is_some_and(|s| !s.starts_with('-'))
            {
                2
            } else {
                1
            };
            let mut ordered = Vec::with_capacity(args.len());
            ordered.extend_from_slice(&args[i..i + words]);
            ordered.extend_from_slice(&args[..i]);
            ordered.extend_from_slice(&args[i + words..]);
            ordered
        }
        _ => args.to_vec(),
    }
}

pub(crate) fn parse(args: &[String]) -> Result<Invocation> {
    let mut common = Common::default();
    let mut rest = Vec::new();
    let mut group = None;
    let mut help = false;
    let mut i = 0;
    let mut literal = false;
    while i < args.len() {
        let arg = &args[i];
        if literal {
            rest.push(arg.clone());
            i += 1;
            continue;
        }
        if arg == "--" {
            literal = true;
            rest.push(arg.clone());
            i += 1;
            continue;
        }
        let (name, inline) = arg
            .split_once('=')
            .map(|(a, b)| (a, Some(b.to_owned())))
            .unwrap_or((arg.as_str(), None));
        let is_value = matches!(
            name,
            "--policy"
                | "--account"
                | "--request-id"
                | "--operation-id"
                | "--rail"
                | "--rpc"
                | "--max-payment"
                | "--budget"
                | "--currency"
        );
        if is_value {
            let value = match inline {
                Some(s) => s,
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| Error::usage(format!("{name} requires a value")))?
                }
            };
            if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
                return Err(Error::usage(format!("{name} has an invalid value")));
            }
            match name {
                "--policy" => {
                    validate_selector(&value)?;
                    set_once(&mut common.policy_selector, value, name)?;
                }
                "--account" => set_once(&mut common.account, value, name)?,
                "--request-id" | "--operation-id" => {
                    if !crate::request::request_id_valid(&value) {
                        return Err(Error::usage(
                            "--request-id must contain 8 to 100 letters, digits, periods, underscores, colons or hyphens",
                        ));
                    }
                    set_once(&mut common.operation_id, value, name)?;
                }
                "--rail" => set_once(&mut common.rail, value, name)?,
                "--currency" => {
                    if value.len() > 32
                        || !value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    {
                        return Err(Error::usage("--currency requires a stablecoin symbol"));
                    }
                    if let Some(money) = &mut common.max_debit {
                        money.currency = value.clone();
                    }
                    set_once(&mut common.currency, value, name)?;
                }
                "--rpc" => {
                    public_url(&value)?;
                    set_once(&mut common.rpc_url, value, name)?;
                }
                "--max-payment" | "--budget" => {
                    if common.max_debit.is_some() {
                        return Err(Error::usage("give one payment limit"));
                    }
                    let amount = value.trim_start_matches('$');
                    if amount.is_empty()
                        || !amount.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                        || amount.matches('.').count() > 1
                        || !amount.bytes().any(|b| b.is_ascii_digit())
                    {
                        return Err(Error::usage("--max-payment requires a decimal USD amount"));
                    }
                    common.max_debit = Some(Money {
                        currency: common.currency.clone().unwrap_or_else(|| "USDC".into()),
                        amount: amount.into(),
                    });
                }
                _ => unreachable!(),
            }
        } else if name == "--sandbox" {
            if inline.is_some() {
                return Err(Error::usage("--sandbox does not take a value"));
            }
            common.sandbox = true;
        } else if name == "--json" {
            common.json = true;
        } else if matches!(name, "--help" | "-h") && group.is_some() {
            help = true;
        } else if group.is_none() && GROUPS.contains(&arg.as_str()) {
            group = Some(arg.clone());
        } else if group.is_none() {
            return Err(Error::usage(
                "expected an Allowit command after global flags",
            ));
        } else {
            rest.push(arg.clone());
        }
        i += 1;
    }
    if common.rail.is_some() != common.rpc_url.is_some() {
        return Err(Error::usage("provide --rail and the exact --rpc together"));
    }
    Ok(Invocation {
        group: group.ok_or_else(|| Error::usage("expected an Allowit command"))?,
        args: rest,
        common,
        help,
    })
}

fn set_once(slot: &mut Option<String>, value: String, name: &str) -> Result<()> {
    if slot.is_some() {
        return Err(Error::usage(format!("{name} may be given only once")));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn validate_selector(value: &str) -> Result<()> {
    let Some((version, address)) = value.split_once('@') else {
        return Err(Error::usage("--policy requires version@addr"));
    };
    if version.is_empty()
        || version.len() > 128
        || address.is_empty()
        || address.len() > 256
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        || !address
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
    {
        return Err(Error::usage(
            "--policy requires one bounded version@addr selector",
        ));
    }
    Ok(())
}

pub(crate) fn public_url(raw: &str) -> Result<Url> {
    let u =
        Url::parse(raw).map_err(|_| Error::usage("request URL must be an absolute HTTP(S) URL"))?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
    {
        return Err(Error::usage(
            "request URL must use HTTP(S), without credentials or a fragment",
        ));
    }
    Ok(u)
}

pub(crate) fn body_file(path: &str) -> Result<String> {
    let meta =
        std::fs::metadata(path).map_err(|_| Error::usage("cannot read the explicit body file"))?;
    if !meta.is_file() || meta.len() > 32 << 10 {
        return Err(Error::usage(
            "body file must be a regular file no larger than 32768 bytes",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|_| Error::usage("cannot open the explicit body file"))?;
    if !file
        .metadata()
        .map_err(|_| Error::usage("cannot inspect body file"))?
        .is_file()
    {
        return Err(Error::usage("body path must remain a regular file"));
    }
    let mut bytes = Vec::new();
    file.take((32 << 10) + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::usage("cannot read body file"))?;
    if bytes.len() > 32 << 10 {
        return Err(Error::usage("body file exceeds 32768 bytes"));
    }
    String::from_utf8(bytes).map_err(|_| Error::usage("this HTTP request body requires UTF-8 text"))
}

pub(crate) fn context(common: &Common) -> Value {
    let mut v = json!({"sandbox":common.sandbox});
    if let Some(n) = crate::routing::selected()
        && let Ok(n) = crate::routing::caip2(&n)
    {
        v["network"] = json!(n);
    }
    for (k, val) in [
        ("policySelector", &common.policy_selector),
        ("account", &common.account),
        ("rail", &common.rail),
        ("rpcUrl", &common.rpc_url),
    ] {
        if let Some(s) = val {
            v[k] = json!(s);
        }
    }
    if let Some(m) = &common.max_debit {
        v["maxDebitMoney"] = json!(m);
    }
    v
}

pub(crate) fn query_context(common: &Common) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in [
        ("account", &common.account),
        ("policySelector", &common.policy_selector),
        ("rail", &common.rail),
        ("rpcUrl", &common.rpc_url),
    ] {
        if let Some(s) = v {
            query.append_pair(k, s);
        }
    }
    query.append_pair("sandbox", if common.sandbox { "true" } else { "false" });
    query.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn policy_is_explicit_version_selector() {
        let v = parse(&args(&[
            "call",
            "hash",
            "--policy",
            "v1@wallet",
            "--sandbox",
        ]))
        .unwrap();
        assert_eq!(v.common.policy_selector.as_deref(), Some("v1@wallet"));
        assert!(v.common.sandbox);
        assert!(parse(&args(&["call", "hash", "--policy", "policy-id"])).is_err());
        assert!(
            parse(&args(&["call", "hash"]))
                .unwrap()
                .common
                .policy_selector
                .is_none()
        );
    }
    #[test]
    fn global_flags_route_by_command_name() {
        assert!(!recognizes(&args(&["--json", "show", "policy"])));
        assert!(!recognizes(&args(&[
            "--request-id",
            "request-01",
            "exec",
            "p"
        ])));
        assert!(recognizes(&args(&["--json", "call", "hash"])));
        assert!(recognizes(&args(&[
            "--policy=v1@wallet",
            "--sandbox",
            "curl"
        ])));
        assert!(recognizes(&args(&["--json"])));
        assert!(!recognizes(&args(&["show", "policy"])));
        assert!(!recognizes(&args(&["--json", "subscriptions"])));
    }
    #[test]
    fn local_command_moves_ahead_of_global_flags() {
        assert_eq!(
            local_command_first(&args(&["--json", "show", "policy"])),
            args(&["show", "--json", "policy"])
        );
        assert_eq!(
            local_command_first(&args(&[
                "--network",
                "solana:devnet",
                "--json",
                "status",
                "policy",
                "request-01"
            ])),
            args(&[
                "status",
                "--network",
                "solana:devnet",
                "--json",
                "policy",
                "request-01"
            ])
        );
        assert_eq!(
            local_command_first(&args(&["--json", "policy", "status"])),
            args(&["policy", "status", "--json"])
        );
        assert_eq!(
            local_command_first(&args(&["--json", "policy"])),
            args(&["policy", "--json"])
        );
        let group = args(&["--json", "call", "hash"]);
        assert_eq!(local_command_first(&group), group);
        let plain = args(&["show", "--json", "policy"]);
        assert_eq!(local_command_first(&plain), plain);
    }
}
