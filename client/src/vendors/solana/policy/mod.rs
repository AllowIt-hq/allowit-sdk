//! Local policy configuration and presentation. Chain and journal semantics belong
//! to the pinned SDK library; this module never launches another command process.
mod deploy;
mod generate;
mod import;
mod lifecycle;
mod status;

use crate::{
    config::env_value,
    error::{Error, Result},
    vendors::solana::audit::{AuditConfig, AuditedRpc, Reporter},
};
use allowit_native::{
    client::{Config, Deployment, NativeClient},
    crypto::{Key, LocalSigner},
    journal::FileJournal,
    lifecycle::{PolicyLifecycle, Record},
    native::{ExecutionRequestIdentity, Options},
    policy::{Policy, genesis},
    rpc::HttpRpc,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Context {
    policy_id: String,
    owner: Key,
    network: String,
    mint: Key,
    executor: Key,
    authority: Key,
    deployment: Deployment,
}
#[derive(Deserialize)]
struct Bundle {
    version: u32,
    policy: Policy,
    context: Context,
    #[serde(default)]
    audit: Option<AuditConfig>,
}
struct Local {
    directory: PathBuf,
    policy_file: PathBuf,
    context: Option<Context>,
    network: String,
    config: Config,
    rpc_url: String,
    audit: Option<AuditConfig>,
}
impl Local {
    fn load(name: &str) -> Result<Self> {
        let directory = absolute(&configured("ALLOWIT_POLICY_DIR", ".allowit"))?;
        let policy_file = absolute(&configured(
            "ALLOWIT_POLICY_FILE",
            directory.join("policy.json").to_string_lossy().as_ref(),
        ))?;
        let context: Option<Context> = read_optional(&directory.join("context.json"))?;
        if let Some(c) = &context {
            for (name, value) in [
                ("ALLOWIT_NETWORK", c.network.clone()),
                ("ALLOWIT_MINT", c.mint.to_string()),
                ("ALLOWIT_EXECUTOR", c.executor.to_string()),
                ("ALLOWIT_AUTHORITY", c.authority.to_string()),
            ] {
                let present = env_value(name);
                if !present.is_empty() && present != value {
                    return Err(Error::config(format!(
                        "Environment {name} differs from the imported policy context"
                    )));
                }
            }
        }
        let network = configured(
            "ALLOWIT_NETWORK",
            context
                .as_ref()
                .map(|c| c.network.as_str())
                .unwrap_or("solana:testnet"),
        );
        let network = crate::routing::normalize(&network)?.to_owned();
        native(genesis(&network))?;
        let mint = if matches!(name, "generate" | "import") {
            context.as_ref().map(|c| c.mint)
        } else {
            public_setting("ALLOWIT_MINT", context.as_ref().map(|c| c.mint))?
        };
        let executor = if matches!(name, "generate" | "import") {
            context.as_ref().map(|c| c.executor)
        } else {
            public_setting("ALLOWIT_EXECUTOR", context.as_ref().map(|c| c.executor))?
        };
        let authority = if matches!(name, "generate" | "import") {
            context.as_ref().map(|c| c.authority)
        } else {
            public_setting("ALLOWIT_AUTHORITY", context.as_ref().map(|c| c.authority))?
        };
        let rpc_url = configured(
            "ALLOWIT_RPC_URL",
            if network == "solana:devnet" {
                "https://api.devnet.solana.com"
            } else {
                "https://api.testnet.solana.com"
            },
        );
        let config = Config {
            network: network.clone(),
            mint,
            executor,
            authority,
            deployment: context.as_ref().map(|c| c.deployment.clone()),
        };
        let journal = FileJournal::new(directory.join("journal"));
        if directory.join("journal").exists() {
            native(journal.initialize())?;
        }
        let audit: Option<AuditConfig> = native(journal.read("audit"))?;
        if let Some(audit) = &audit {
            audit.validate()?;
        }
        Ok(Self {
            directory,
            policy_file,
            context,
            network,
            config,
            rpc_url,
            audit,
        })
    }
    fn client(
        &self,
        policy_id: &str,
        audit_enabled: bool,
    ) -> Result<(NativeClient, Option<Arc<Reporter>>)> {
        let rpc = native(HttpRpc::new(&self.rpc_url))?;
        let audit = if audit_enabled {
            self.audit
                .as_ref()
                .map(|config| native(Reporter::new(config.clone(), policy_id.into())))
                .transpose()?
                .map(Arc::new)
        } else {
            None
        };
        let rpc: Arc<dyn allowit_native::rpc::Rpc> = if let Some(reporter) = &audit {
            Arc::new(AuditedRpc {
                rpc: Box::new(rpc),
                reporter: reporter.clone(),
                journal: FileJournal::new(self.directory.join("journal")),
            })
        } else {
            Arc::new(rpc)
        };
        let client = native(NativeClient::new(self.config.clone(), rpc))?;
        Ok((client, audit))
    }
    fn save_policy(&self, policy: &Policy) -> Result<()> {
        create_private(
            &self.policy_file,
            policy,
            "A policy already exists here. Choose a new ALLOWIT_POLICY_DIR; keep the previous policy and journal for recovery.",
        )
    }
}
/// Saved policy state shared by status and the owner/executor lifecycle operations.
struct Session<'a> {
    local: &'a Local,
    policy: &'a Policy,
    owner: Key,
    signer: Option<&'a LocalSigner>,
    client: &'a NativeClient,
    audit: Option<&'a Reporter>,
    journal: &'a FileJournal,
    lifecycle: &'a PolicyLifecycle<'a>,
}
pub(crate) fn run(
    name: &str,
    pos: &[String],
    json: bool,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    native(allowit_native::journal::require_supported_platform())?;
    let mut local = Local::load(name)?;
    match name {
        "generate" => return generate::run(&local, pos, json, stdout),
        "import" => return import::run(&local, pos, json, stdout),
        _ => {}
    }
    let policy: Policy = read_optional(&local.policy_file)?
        .ok_or_else(|| Error::config("Cannot read saved policy"))?;
    native(policy.validate())?;
    if local
        .context
        .as_ref()
        .is_some_and(|c| c.policy_id != policy.id)
    {
        return Err(Error::config(
            "Saved context belongs to a different policy instance",
        ));
    }
    let deployment_file = env_value("ALLOWIT_DEPLOYMENT_FILE");
    if !deployment_file.is_empty() {
        let d: Deployment = read_optional(Path::new(&deployment_file))?
            .ok_or_else(|| Error::config("Cannot read native deployment"))?;
        if local.context.as_ref().is_some_and(|c| {
            serde_json::to_vec(&c.deployment).unwrap() != serde_json::to_vec(&d).unwrap()
        }) {
            return Err(Error::config(
                "Deployment differs from the imported policy context",
            ));
        }
        local.config.deployment = Some(d);
    }
    if local.config.deployment.is_none() {
        return Err(Error::config(
            "Configure ALLOWIT_DEPLOYMENT_FILE or import the executor bundle",
        ));
    }
    if name == "deploy" {
        return deploy::run(&local, &policy, pos, json, stdout);
    }
    let owner_role = !matches!(name, "execute" | "status");
    let signer = if owner_role {
        Some(load_signer("ALLOWIT_OWNER_KEYPAIR")?)
    } else {
        None
    };
    let owner=match &signer {Some(s)=>s.public_key(),None=>public_setting("ALLOWIT_OWNER",local.context.as_ref().map(|c|c.owner))?.ok_or_else(||Error::config("Configure ALLOWIT_OWNER with the vault owner public key for execute/status; no owner key is needed"))?};
    if local.context.as_ref().is_some_and(|c| c.owner != owner) {
        return Err(Error::config(
            "Configured owner differs from the imported policy",
        ));
    }
    let (client, audit) = local.client(&policy.id, matches!(name, "execute" | "status"))?;
    let journal = FileJournal::new(local.directory.join("journal"));
    if owner_role && local.context.is_none() {
        let bundle = native(client.bundle(&policy, owner))?;
        let context: Context = serde_json::from_value(bundle["context"].clone())
            .map_err(|_| Error::config("Invalid generated policy context"))?;
        native(journal.locked(|| {
            create_private(
                &local.directory.join("context.json"),
                &context,
                "A public context already exists here; recover it before continuing",
            )
            .map_err(to_native)
        }))?;
    }
    let lifecycle = PolicyLifecycle::new(&client, &journal);
    let session = Session {
        local: &local,
        policy: &policy,
        owner,
        signer: signer.as_ref(),
        client: &client,
        audit: audit.as_deref(),
        journal: &journal,
        lifecycle: &lifecycle,
    };
    if name == "status" {
        return status::run(&session, json, stdout, stderr);
    }
    lifecycle::run(&session, name, pos, json, stdout, stderr)
}

fn load_signer(name: &str) -> Result<LocalSigner> {
    let path = env_value(name);
    if path.is_empty() {
        return Err(Error::config(format!(
            "Configure {name} with a dedicated test-network key file"
        )));
    }
    native(LocalSigner::load(Path::new(&path)))
}
fn save_text(path: &Path, text: &str) -> Result<()> {
    // Atomic replacement avoids following a pre-existing skill symlink.
    let parent = path
        .parent()
        .ok_or_else(|| Error::config("Invalid skill path"))?;
    let temp = parent.join(format!(".skill-{}", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|_| Error::config("Cannot save generated skill"))?;
    let result = (|| {
        file.write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| Error::config("Cannot persist generated skill"))?;
        drop(file);
        fs::rename(&temp, path).map_err(|_| Error::config("Cannot save generated skill"))?;
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::config("Cannot persist generated skill directory"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
struct OfflineRpc;
impl allowit_native::rpc::Rpc for OfflineRpc {
    fn call(
        &self,
        _: &str,
        _: serde_json::Value,
    ) -> allowit_native::error::Result<serde_json::Value> {
        Err(allowit_native::error::Error::config(
            "Offline policy operation cannot access RPC",
        ))
    }
}

fn configured(name: &str, fallback: &str) -> String {
    let v = env_value(name);
    if v.is_empty() { fallback.into() } else { v }
}
fn execution_context() -> Result<serde_json::Value> {
    let inline = env_value("ALLOWIT_EXECUTION_CONTEXT_JSON");
    let file = env_value("ALLOWIT_EXECUTION_CONTEXT_FILE");
    if !inline.is_empty() && !file.is_empty() {
        return Err(Error::config("Configure only one execution context source"));
    }
    let context = if !file.is_empty() {
        read_optional(Path::new(&file))?
            .ok_or_else(|| Error::config("Cannot read execution context file"))?
    } else if inline.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&inline)
            .map_err(|_| Error::config("Execution context must be a JSON object"))?
    };
    if !context.is_object() {
        return Err(Error::config("Execution context must be a JSON object"));
    }
    Ok(context)
}
fn public_setting(name: &str, fallback: Option<Key>) -> Result<Option<Key>> {
    let v = env_value(name);
    if v.is_empty() {
        Ok(fallback)
    } else {
        native(Key::parse(&v)).map(Some)
    }
}
fn absolute(path: &str) -> Result<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        Ok(p)
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .map_err(|_| Error::config("Cannot locate policy directory"))
    }
}
fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::config("Unreadable policy context or file")),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(2_097_153)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::config("Unreadable policy context or file"))?;
    if bytes.len() > 2_097_152 {
        return Err(Error::config("Policy file exceeded its size limit"));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::config("Unreadable policy context or file"))
}
fn create_private(path: &Path, value: &impl Serialize, occupied: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::config("Invalid policy path"))?;
    let mut dirs = fs::DirBuilder::new();
    dirs.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        dirs.mode(0o700);
    }
    dirs.create(parent)
        .map_err(|_| Error::config("Cannot create policy directory"))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| {
        Error::config(if e.kind() == std::io::ErrorKind::AlreadyExists {
            occupied
        } else {
            "Cannot save policy file"
        })
    })?;
    serde_json::to_writer_pretty(&mut file, value)
        .map_err(|_| Error::config("Cannot serialize policy file"))?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::config("Cannot persist policy file"))?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::config("Cannot persist policy directory"))
}
fn native<T>(result: allowit_native::error::Result<T>) -> Result<T> {
    result.map_err(|e| Error {
        code: e.code,
        message: e.message,
        status: None,
        config: e.code == 3,
    })
}
fn to_native(e: Error) -> allowit_native::error::Error {
    allowit_native::error::Error {
        code: e.code,
        message: e.message,
    }
}
