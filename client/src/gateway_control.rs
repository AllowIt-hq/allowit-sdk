//! Public account registration and service controls. Blockchain agent keys stay local.
use crate::{
    error::{Error, Result},
    gateway::{Gateway, operation_id, render_operation},
    gateway_args::{self, Invocation},
    output,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Component, Path, PathBuf},
};

fn input(args: &[String]) -> Result<(Vec<String>, Value)> {
    let mut positions = vec![];
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
        } else if matches!(
            a.as_str(),
            "--fee-within" | "--confidential" | "--changed-only"
        ) {
            value[a.trim_start_matches('-')] = json!(true);
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
            if ![
                "name",
                "network",
                "to",
                "amount",
                "currency",
                "source",
                "service",
                "query",
                "limit",
                "subscription",
                "port",
                "upstream",
                "model",
                "config",
                "memo",
                "memo-hex",
                "changed-from",
                "registry",
                "fqn",
                "openapi-url",
            ]
            .contains(&name)
            {
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
            positions.push(a.clone());
        }
        i += 1;
    }
    Ok((positions, value))
}

/// Keep account controls separate from arbitrary service input.
/// An allowlist prevents future key fields from silently becoming transport data.
fn public_account_input(command: &str, action: &str, input: &Value, pos: &[String]) -> Result<()> {
    if !matches!(command, "account" | "setup" | "send" | "topup" | "whoami") {
        return Ok(());
    }
    if command == "account" && matches!(action, "new" | "import" | "export") {
        return Err(Error::usage(
            "Blockchain wallet keys stay on this device. Use allowit wallet generate --network NETWORK --output PRIVATE_FILE, then register its public metadata with allowit setup. Remote key creation, import and export are refused",
        ));
    }
    let fields = input
        .as_object()
        .ok_or_else(|| Error::usage("Account controls require a public JSON object"))?;
    let allowed: &[&str] = match command {
        "setup" => &["network", "address", "publicKey", "keyType", "name"],
        "send" | "topup" => &[
            "network",
            "to",
            "amount",
            "currency",
            "name",
            "memo",
            "memo-hex",
            "fee-within",
        ],
        "account" if action != "list" => &["name"],
        _ => &[],
    };
    if fields.keys().any(|name| !allowed.contains(&name.as_str())) {
        return Err(Error::usage(
            "Account input contains a field outside its public schema. Private keys, seeds, shares and wallet files must remain local",
        ));
    }
    if fields.iter().any(|(name, value)| {
        if name == "fee-within" {
            !value.is_boolean()
        } else {
            value.as_str().is_none_or(|text| {
                text.is_empty() || text.len() > 2048 || text.chars().any(char::is_control)
            })
        }
    }) {
        return Err(Error::usage(
            "Account public fields must contain bounded text",
        ));
    }
    if command == "account" {
        let expected = if action == "list" { 1 } else { 2 };
        // An omitted action means list. Never forward an extra positional as a query.
        if pos.len() > expected {
            return Err(Error::usage("Unexpected account argument"));
        }
        if action != "list" && !fields.contains_key("name") && pos.get(1).is_none() {
            return Err(Error::usage("Select the public account name"));
        }
    }
    if command == "setup" {
        let mut registration = input.clone();
        registration.as_object_mut().unwrap().remove("name");
        let wallet: crate::wallet::PublicWallet =
            serde_json::from_value(registration).map_err(|_| {
                Error::usage(
                    "Setup requires network, address, publicKey and keyType from the local wallet",
                )
            })?;
        wallet.validate()?;
        if crate::routing::caip2(&wallet.network)? != wallet.network {
            return Err(Error::usage(
                "Wallet registration requires its canonical x402 network identifier",
            ));
        }
        if crate::routing::selected().is_some_and(|network| {
            crate::routing::normalize(&wallet.network).is_ok_and(|bound| bound != network)
        }) {
            return Err(Error::usage("Wallet registration conflicts with --network"));
        }
    }
    Ok(())
}

pub(crate) fn run(inv: &Invocation, stdout: &mut String, stderr: &mut String) -> Result<i32> {
    let mut args = vec![];
    let mut output_dir = None;
    let mut force = false;
    let mut n = 0;
    while n < inv.args.len() {
        if inv.args[n] == "--output-dir" {
            n += 1;
            output_dir =
                Some(PathBuf::from(inv.args.get(n).ok_or_else(|| {
                    Error::usage("--output-dir requires a directory")
                })?));
        } else if inv.args[n] == "--force" {
            force = true;
        } else {
            args.push(inv.args[n].clone());
        }
        n += 1;
    }
    let (pos, mut input) = input(&args)?;
    let command = inv.group.as_str();
    if let Some(currency) = &inv.common.currency {
        input["currency"] = json!(currency);
    }
    let read = |route: &str| -> Result<Value> {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in [
            ("account", &inv.common.account),
            ("policySelector", &inv.common.policy_selector),
            ("rail", &inv.common.rail),
            ("rpcUrl", &inv.common.rpc_url),
        ] {
            if let Some(s) = v {
                query.append_pair(k, s);
            }
        }
        query.append_pair("sandbox", if inv.common.sandbox { "true" } else { "false" });
        if let Some(s) = pos.get(1) {
            query.append_pair("query", s);
        }
        let q = query.finish();
        Gateway::new(&inv.common)?.send("GET", &format!("{route}?{q}"), None)
    };
    let (route, action, is_read) = match command {
        "whoami" => {
            if !pos.is_empty() {
                return Err(Error::usage("usage: allowit whoami"));
            }
            ("/api/gateway/whoami", "whoami", true)
        }
        "account" => {
            let a = pos.first().map(String::as_str).unwrap_or("list");
            let a = match a {
                "ls" => "list",
                "rm" | "destroy" => "remove",
                "backup" => "export",
                s => s,
            };
            if !["list", "new", "import", "default", "remove", "export"].contains(&a) {
                return Err(Error::usage("unknown account action"));
            }
            ("/api/gateway/account", a, a == "list")
        }
        "setup" | "send" | "topup" => {
            if !pos.is_empty() {
                if command == "send" && pos.len() == 2 {
                    input["amount"] = json!(pos[0]);
                    input["to"] = json!(pos[1]);
                } else if command == "topup" && pos.len() == 1 {
                    input["amount"] = json!(pos[0]);
                } else {
                    return Err(Error::usage(
                        "use --input JSON or named fields for this account operation",
                    ));
                }
            }
            (
                match command {
                    "setup" => "/api/gateway/setup",
                    "send" => "/api/gateway/send",
                    _ => "/api/gateway/topup",
                },
                command,
                false,
            )
        }
        "subscriptions" => {
            let a = pos.first().map(String::as_str).unwrap_or("list");
            if !["list", "new", "cancel", "status"].contains(&a) {
                return Err(Error::usage("unknown subscriptions action"));
            }
            ("/api/gateway/subscriptions", a, a == "list")
        }
        "skills" => {
            let a = pos.first().map(String::as_str).unwrap_or("list");
            let a = match a {
                "ls" => "list",
                "rm" => "remove",
                "endpoints" => "show",
                s => s,
            };
            if ![
                "search",
                "show",
                "add",
                "remove",
                "list",
                "update",
                "prune-merged",
                "provider",
            ]
            .contains(&a)
            {
                return Err(Error::usage("unknown skills action"));
            }
            (
                if matches!(a, "search" | "show" | "list") {
                    "/api/gateway/services"
                } else {
                    "/api/gateway/skills"
                },
                a,
                matches!(a, "search" | "show" | "list"),
            )
        }
        "catalog" => {
            let a = pos.first().map(String::as_str).unwrap_or("list");
            if !["list", "search", "show", "scaffold", "check", "build"].contains(&a) {
                return Err(Error::usage("unknown catalog action"));
            }
            (
                if matches!(a, "list" | "search" | "show") {
                    "/api/gateway/services"
                } else {
                    "/api/gateway/catalog"
                },
                a,
                matches!(a, "list" | "search" | "show"),
            )
        }
        "install" => {
            if pos.len() != 1 {
                return Err(Error::usage("usage: allowit install SOURCE"));
            }
            input["source"] = json!(pos[0]);
            ("/api/gateway/skills", "add", false)
        }
        _ => return Err(Error::usage("unknown Allowit control command")),
    };
    if inv.common.policy_selector.is_some()
        && (matches!(
            command,
            "account" | "whoami" | "setup" | "send" | "topup" | "subscriptions" | "install"
        ) || (!is_read && matches!(command, "skills" | "catalog")))
    {
        return Err(Error::usage(
            "this owner/account command requires direct account authority, without --policy",
        ));
    }
    public_account_input(command, action, &input, &pos)?;
    if is_read {
        let value = read(route)?;
        *stdout += &output::json(&value);
        return Ok(0);
    }
    if command == "catalog" && action == "scaffold" {
        if pos.len() != 3 {
            return Err(Error::usage(
                "usage: allowit catalog scaffold SELLER/RESOURCE OPENAPI_URL [--output-dir DIR]",
            ));
        }
        gateway_args::public_url(&pos[2])?;
        input["fqn"] = json!(pos[1]);
        input["openapiUrl"] = json!(pos[2]);
        if output_dir.is_none() {
            output_dir = Some(PathBuf::from("."));
        }
    }
    if let Some(name) = pos.get(1) {
        if matches!(command, "account") {
            input["name"] = json!(name);
        } else if command == "subscriptions" {
            input["subscription"] = json!(name);
        } else if command == "skills" {
            input["source"] = json!(name);
        }
    }
    let mut request = gateway_args::context(&inv.common);
    request["action"] = json!(action);
    request["input"] = input;
    let id = operation_id(&inv.common, &request);
    request["operationId"] = json!(id);
    *stderr += &format!(
        "allowit: {command} operation {id}; do not repeat an uncertain mutation with a new operation ID\n"
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
    if let Some(dir) = output_dir {
        write_files(&value, &dir, force)?;
    }
    *stdout += &output::json(&value);
    Ok(0)
}

fn write_files(value: &Value, dir: &Path, force: bool) -> Result<()> {
    let files = value
        .get("files")
        .and_then(Value::as_array)
        .or_else(|| {
            value
                .get("result")
                .and_then(|v| v.get("files"))
                .and_then(Value::as_array)
        })
        .ok_or_else(|| Error::unsupported("backend response contains no generated files"))?;
    if files.len() > 512 {
        return Err(Error::unsupported("backend generated too many files"));
    }
    let mut entries = vec![];
    let mut total = 0;
    for f in files {
        let path = f["path"]
            .as_str()
            .ok_or_else(|| Error::unsupported("generated file lacks a relative path"))?;
        let content = f["content"]
            .as_str()
            .ok_or_else(|| Error::unsupported("generated file lacks text content"))?;
        let p = Path::new(path);
        if p.as_os_str().is_empty()
            || p.components().any(|c| !matches!(c, Component::Normal(_)))
            || content.len() > 256 << 10
        {
            return Err(Error::unsupported(
                "generated file path or content is outside bounds",
            ));
        }
        total += content.len();
        entries.push((p, content));
    }
    if total > 2 << 20 {
        return Err(Error::unsupported("generated files exceed 2 MB"));
    }
    std::fs::create_dir_all(dir)
        .map_err(|_| Error::config("cannot create selected output directory"))?;
    let root =
        std::fs::canonicalize(dir).map_err(|_| Error::config("cannot inspect output directory"))?;
    for (path, content) in entries {
        let dest = root.join(path);
        let parent = dest
            .parent()
            .ok_or_else(|| Error::config("generated path has no parent"))?;
        let mut partial = root.clone();
        for part in path.parent().unwrap_or(Path::new("")).components() {
            partial.push(part);
            if std::fs::symlink_metadata(&partial).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(Error::config("generated output cannot cross a symlink"));
            }
        }
        std::fs::create_dir_all(parent)
            .map_err(|_| Error::config("cannot create generated file directory"))?;
        if !std::fs::canonicalize(parent)
            .map_err(|_| Error::config("cannot inspect generated file directory"))?
            .starts_with(&root)
        {
            return Err(Error::config("generated output escaped selected directory"));
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        if force {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options.open(dest).map_err(|_| {
            Error::config(
                "cannot create generated file; use --force only to replace selected files",
            )
        })?;
        if !file
            .metadata()
            .map_err(|_| Error::config("cannot inspect generated file"))?
            .is_file()
        {
            return Err(Error::config("generated output must be a regular file"));
        }
        file.write_all(content.as_bytes())
            .map_err(|_| Error::config("cannot write generated file"))?;
    }
    Ok(())
}

#[cfg(test)]
mod key_boundary_tests {
    use super::*;

    fn registration() -> Value {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .to_bytes();
        let public = bs58::encode(key).into_string();
        json!({
            "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
            "address": public,
            "publicKey": public,
            "keyType": "ed25519"
        })
    }

    #[test]
    fn remote_key_lifecycle_is_refused_before_transport_or_authentication() {
        for action in ["new", "import", "export", "backup"] {
            let inv = gateway_args::parse(&["account".into(), action.into()]).unwrap();
            let mut stdout = String::new();
            let mut stderr = String::new();
            let error = run(&inv, &mut stdout, &mut stderr).unwrap_err();
            assert_eq!(error.code, 2);
            assert!(error.message.contains("keys stay on this device"));
            assert!(stdout.is_empty());
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn setup_accepts_only_matching_public_wallet_metadata() {
        let public = registration();
        assert!(public_account_input("setup", "setup", &public, &[]).is_ok());
        let mut wrong = public.clone();
        wrong["address"] = json!("11111111111111111111111111111111");
        assert!(public_account_input("setup", "setup", &wrong, &[]).is_err());
        for field in [
            "seed",
            "privateKey",
            "keypair",
            "agentShare",
            "walletFile",
            "config",
        ] {
            let mut secret = public.clone();
            secret[field] = json!({"value": "private material"});
            assert!(public_account_input("setup", "setup", &secret, &[]).is_err());
        }
        assert!(public_account_input("setup", "setup", &json!({}), &[]).is_err());
    }

    #[test]
    fn public_registration_cannot_change_the_selected_network() {
        crate::routing::with_network(Some("tempo:testnet".into()), || {
            assert!(public_account_input("setup", "setup", &registration(), &[]).is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn transfers_reject_secret_fields_and_nested_configuration() {
        for command in ["send", "topup"] {
            let public = json!({"amount":"1", "to":"public-recipient", "currency":"USDC"});
            assert!(public_account_input(command, command, &public, &[]).is_ok());
            for payload in [
                json!({"amount":"1", "seed":"private material"}),
                json!({"amount":"1", "to":{"privateKey":"private material"}}),
                json!({"amount":"1", "config":{"agentShare":"private material"}}),
            ] {
                assert!(public_account_input(command, command, &payload, &[]).is_err());
            }
        }
    }

    #[test]
    fn account_reads_do_not_forward_unused_input_or_query() {
        assert!(public_account_input("account", "list", &json!({}), &[]).is_ok());
        assert!(
            public_account_input(
                "account",
                "list",
                &json!({}),
                &["list".into(), "unintended-query".into()]
            )
            .is_err()
        );
        assert!(public_account_input("account", "list", &json!({"seed":"private"}), &[]).is_err());
        assert!(
            public_account_input("account", "default", &json!({"name":"local-wallet"}), &[])
                .is_ok()
        );
    }
}
