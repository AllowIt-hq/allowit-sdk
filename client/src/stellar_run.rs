//! Existing exec/status transport for an explicitly installed Stellar wallet profile.
use crate::{
    args::Args,
    config::{env_value, parse_origin},
    error::{Error, Result},
    typed_request::{decode_bytes, operation_nonce},
    typed_run::{TypedInput, read_typed_file},
};
use allowit_client_core::store::Store;
use allowit_policy_sdk::typed_workflow::ExecutionRequest;
use allowit_stellar_wallet::{
    self as sdk, DetachedSignature, OperationStatus, PreparedOperation, SignedOperation,
    WalletConfig, WalletRoute,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    io::Read,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Bundle {
    version: u8,
    profile: String,
    network: String,
    policy_id: String,
    source_hash: String,
    ir_hash: String,
    module_digest: String,
    wallet: String,
    route: WalletRoute,
    config: WalletConfig,
    endpoints: Endpoints,
    /// Customer secret. This field is decoded locally and never serialized or sent.
    #[serde(default, deserialize_with = "local_secret")]
    signing_seed: Option<Zeroizing<String>>,
}
fn local_secret<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<Zeroizing<String>>, D::Error> {
    String::deserialize(decoder).map(|value| Some(Zeroizing::new(value)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoints {
    execute: String,
    status: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Response {
    prepared: PreparedOperation,
    operation: Operation,
    signed_operation: Option<SignedOperation>,
    receipt: Option<sdk::rpc::VerifiedReceipt>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Operation {
    id: String,
    status: OperationStatus,
    transaction_hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Record {
    policy_id: String,
    request_id: String,
    config: WalletConfig,
    request: ExecutionRequest,
    prepared: Option<PreparedOperation>,
    agent_signature: Option<DetachedSignature>,
    status: Option<OperationStatus>,
}
struct Client {
    http: reqwest::blocking::Client,
    origin: String,
    bundle: Bundle,
    seed: Zeroizing<[u8; 32]>,
}
fn revalidate_for_signing(p: &PreparedOperation, now: u64) -> Result<PreparedOperation> {
    revalidate_with_rpc_url(p, now, &env_value("ALLOWIT_STELLAR_RPC_URL"))
}
fn revalidate_with_rpc_url(
    p: &PreparedOperation,
    now: u64,
    value: &str,
) -> Result<PreparedOperation> {
    if p.config.route == WalletRoute::Native {
        return Ok(p.clone());
    }
    if value.is_empty() {
        return Err(Error::config(
            "Set local ALLOWIT_STELLAR_RPC_URL before new policy wallet signing",
        ));
    }
    let rpc = checked(sdk::rpc::HttpRpc::new(value))?;
    checked(sdk::rpc::revalidate_prepared(&rpc, p, now))
}
fn clock() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::config("System clock precedes the Unix epoch"))
}
/// The backend may add only signed accounting facts to the original caller intent.
/// Exact prepared bytes remain pinned separately and require trusted native revalidation before signing.
fn same_execution_intent(a: &ExecutionRequest, b: &ExecutionRequest) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    a.expense = None;
    b.expense = None;
    a == b
}
fn checked<T>(v: sdk::Result<T>) -> Result<T> {
    v.map_err(Error::config)
}
fn journal_error(_: Error) -> Error {
    Error::config("Stellar operation journal is unsafe or unreadable; retain the original journal")
}
fn same_policy_config(expected: &WalletConfig, actual: &WalletConfig) -> bool {
    let mut current = actual.clone();
    // The server signer is checked against the prepared live chain state, not frozen token metadata.
    current.server = expected.server.clone();
    &current == expected
}
fn same_retained_policy_config(expected: &WalletConfig, actual: &WalletConfig) -> bool {
    let mut original = actual.clone();
    // Public role identities may rotate. Read-only recovery still binds the original approved source and exact packet.
    original.agent = expected.agent.clone();
    same_policy_config(expected, &original)
}
fn validate_bundle(policy: &str, bundle: &Bundle) -> Result<()> {
    if bundle.version != 1
        || bundle.profile != sdk::PROFILE
        || bundle.policy_id != policy
        || bundle.network != bundle.config.network
        || bundle.wallet != bundle.config.wallet
        || bundle.route != bundle.config.route
        || checked(sdk::unhex::<32>(&bundle.source_hash))? != bundle.config.source_hash
        || checked(sdk::unhex::<32>(&bundle.module_digest))? != bundle.config.module_digest
        || sdk::unhex::<32>(&bundle.ir_hash).is_err()
        || bundle.endpoints.execute != "/api/native/execute"
        || bundle.endpoints.status != "/api/native/execute/status"
    {
        return Err(Error::config(
            "The Stellar bundle does not match the installed policy profile",
        ));
    }
    checked(sdk::validate_config(&bundle.config))
}
fn read_public_bundle(path: &Path) -> Result<Bundle> {
    let mut bundle: Bundle = crate::typed_request::read_request_file(path)?;
    if let Some(_private) = bundle.signing_seed.take() {
        return Err(Error::config(
            "The public Stellar bundle must not contain signing material",
        ));
    }
    validate_bundle(&bundle.policy_id, &bundle)?;
    if bundle.config.owner == bundle.config.agent
        || bundle.config.owner == bundle.config.server
        || bundle.config.agent == bundle.config.server
    {
        return Err(Error::config(
            "The public Stellar wallet must use distinct owner, agent and server identities",
        ));
    }
    Ok(bundle)
}
pub(crate) fn public_binding(path: &Path) -> Result<(String, String)> {
    let bundle = read_public_bundle(path)?;
    Ok((
        bundle.policy_id,
        crate::routing::normalize(&bundle.network)?.into(),
    ))
}
fn public_description(bundle: &Bundle, source: bool) -> serde_json::Value {
    let mut value = json!({
        "policyId":bundle.policy_id,"profile":bundle.profile,"network":bundle.network,
        "wallet":bundle.wallet,"route":bundle.route,"owner":bundle.config.owner,
        "agent":bundle.config.agent,"server":bundle.config.server,
        "sourceHash":bundle.source_hash,"irHash":bundle.ir_hash,"moduleDigest":bundle.module_digest,
        "asset":bundle.config.asset,"assetSymbol":bundle.config.asset_symbol,"assetIssuer":bundle.config.asset_issuer,
        "maxFeeStroops":bundle.config.max_fee_stroops
    });
    if source {
        value["policy"] = json!(bundle.config.policy_source);
    }
    value
}
/// Inspect only validated public metadata. This operation never opens a signer or a transport.
pub(crate) fn show(policy: &str, source: bool, stdout: &mut String) -> Result<i32> {
    let path = env_value("ALLOWIT_STELLAR_BUNDLE");
    if path.is_empty() {
        return Err(Error::config(
            "Set ALLOWIT_STELLAR_BUNDLE to inspect public wallet metadata",
        ));
    }
    let bundle = read_public_bundle(Path::new(&path))?;
    validate_bundle(policy, &bundle)?;
    *stdout += &crate::output::json(&public_description(&bundle, source));
    Ok(0)
}
impl Client {
    fn load(policy: &str) -> Result<Self> {
        let public_path = env_value("ALLOWIT_STELLAR_BUNDLE");
        if !public_path.is_empty() {
            if !env_value("ALLOWIT_TOKEN").is_empty() {
                return Err(Error::config(
                    "Use either the public Stellar bundle with a local signer or the legacy token, not both",
                ));
            }
            let bundle = read_public_bundle(Path::new(&public_path))?;
            let path = env_value("ALLOWIT_STELLAR_SIGNER");
            if path.is_empty() {
                return Err(Error::config(
                    "Set ALLOWIT_STELLAR_SIGNER to the local private signer file",
                ));
            }
            let seed = crate::wallet::stellar_signing_seed(Path::new(&path))?;
            if sdk::account_for_seed(&seed) != bundle.config.agent {
                return Err(Error::config(
                    "The local Stellar signer differs from the public bundle agent",
                ));
            }
            return Self::from_bundle(policy, bundle, seed);
        }
        let raw = Zeroizing::new(env_value("ALLOWIT_TOKEN"));
        let encoded = raw
            .strip_prefix("allowit-stellar-v1.")
            .ok_or_else(|| Error::config("Set ALLOWIT_TOKEN to the customer-held Stellar token"))?;
        if encoded.len() > 131_072
            || encoded.len() % 2 != 0
            || !encoded.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::config("Invalid Stellar token encoding"));
        }
        let bytes = Zeroizing::new(
            (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>(),
        );
        let mut bundle: Bundle = decode_bytes(&bytes)?;
        let private = bundle
            .signing_seed
            .take()
            .ok_or_else(|| Error::config("The legacy Stellar token lacks its local signer"))?;
        let seed = Zeroizing::new(checked(sdk::decode_seed(&private))?);
        // Signer membership is checked against current chain authority by the service.
        bundle.config.agent = sdk::account_for_seed(&seed);
        Self::from_bundle(policy, bundle, seed)
    }
    fn from_bundle(policy: &str, bundle: Bundle, seed: Zeroizing<[u8; 32]>) -> Result<Self> {
        validate_bundle(policy, &bundle)?;
        if let Some(network) = crate::routing::selected()
            && crate::routing::normalize(&bundle.network)? != network
        {
            return Err(Error::config(
                "Selected network differs from Stellar wallet binding",
            ));
        }
        let origin = parse_origin(&env_value("ALLOWIT_URL"))?;
        let builder = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none());
        Ok(Self {
            http: builder
                .build()
                .map_err(|_| Error::config("Cannot initialize Stellar transport"))?,
            origin,
            bundle,
            seed,
        })
    }
    fn post(&self, path: &str, body: serde_json::Value) -> Result<Response> {
        use ed25519_dalek::{Signer, SigningKey};
        let issued_at = clock()?;
        let canonical = serde_json_canonicalizer::to_vec(&body)
            .map_err(|_| Error::config("Cannot encode Stellar request"))?;
        let mut message = format!("allowit-stellar-request-v1\n{issued_at}\n{path}\n").into_bytes();
        message.extend_from_slice(&canonical);
        let signature = SigningKey::from_bytes(&self.seed).sign(&sdk::hash(message));
        let proof = format!(
            "Bearer stellar-proof.{}.{issued_at}.{}",
            sdk::account_for_seed(&self.seed),
            sdk::hex(&signature.to_bytes())
        );
        let response = self
            .http
            .post(format!("{}{path}", self.origin))
            .header("Authorization", proof)
            .json(&body)
            .send()
            .map_err(|_| {
                Error::uncertain(
                    "Stellar transport is uncertain; recover the original request ID with status",
                )
            })?;
        let code = response.status().as_u16();
        if !(200..300).contains(&code) {
            return Err(Error::api(
                code,
                "Stellar operation was not resolved; retain the original request ID",
            ));
        }
        let mut bytes = Vec::new();
        response
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                Error::uncertain("Cannot read Stellar response; recover the original request ID")
            })?;
        if bytes.len() > 1_048_576 {
            return Err(Error::uncertain(
                "Stellar response exceeds its limit; recover the original request ID",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            Error::uncertain("Invalid Stellar response; recover the original request ID")
        })
    }
    fn validate(
        &self,
        r: &Response,
        id: &str,
        record: Option<&Record>,
        read_only: bool,
    ) -> Result<()> {
        let config_matches = if read_only {
            same_retained_policy_config
        } else {
            same_policy_config
        };
        let p = &r.prepared;
        if r.operation.id != id
            || !config_matches(&self.bundle.config, &p.config)
            || p.request.operation.nonce != operation_nonce(&self.bundle.policy_id, id)
            || r.operation.transaction_hash != sdk::hex(&p.transaction_hash)
            || record.is_some_and(|old| {
                !same_execution_intent(&old.request, &p.request)
                    || !config_matches(&old.config, &p.config)
                    || old.prepared.as_ref().is_some_and(|v| v != p)
            })
        {
            return Err(Error::uncertain(
                "Stellar response changed the approved operation or source. Refresh the owner-approved public bundle before using an updated policy.",
            ));
        }
        if terminal(&r.operation.status) && r.receipt.is_none() {
            return Err(Error::uncertain(
                "Terminal Stellar status requires a verified chain receipt",
            ));
        }
        if let Some(receipt) = &r.receipt {
            checked(sdk::rpc::validate_receipt(
                receipt,
                &p.config.network,
                &p.transaction_hash,
            ))?;
            if receipt.status != r.operation.status
                || r.signed_operation
                    .as_ref()
                    .is_none_or(|v| v.transaction_xdr != receipt.envelope_xdr)
            {
                return Err(Error::uncertain(
                    "Stellar receipt differs from its retained signed operation",
                ));
            }
        }
        // Status verifies retained bytes at their preparation time, including after expiry.
        checked(sdk::validate_retained_prepared(p))?;
        if let Some(s) = &r.signed_operation {
            if s.prepared != *p {
                return Err(Error::uncertain(
                    "Stellar signed packet differs from the original operation",
                ));
            }
            checked(sdk::validate_retained_signed(s))?;
        }
        if matches!(
            r.operation.status,
            OperationStatus::Submitted | OperationStatus::Confirmed | OperationStatus::Failed
        ) && r.signed_operation.is_none()
        {
            return Err(Error::uncertain(
                "Stellar settlement omitted its signed packet",
            ));
        }
        if let Some(old) = record.and_then(|v| v.status.as_ref())
            && matches!(old, OperationStatus::Confirmed | OperationStatus::Failed)
            && *old != r.operation.status
        {
            return Err(Error::uncertain("A terminal Stellar receipt changed"));
        }
        Ok(())
    }
    fn status(&self, id: &str, record: Option<&Record>) -> Result<Response> {
        let r = self.post(
            &self.bundle.endpoints.status,
            json!({"policyId":self.bundle.policy_id,"requestId":id}),
        )?;
        self.validate(&r, id, record, true)?;
        Ok(r)
    }
}
pub(crate) fn run(
    command: &str,
    args: &Args,
    stdout: &mut String,
    stderr: &mut String,
) -> Result<i32> {
    let client = Client::load(&args.pos[0])?;
    let journal = Store::open(Path::new(".allowit/stellar-journal")).map_err(journal_error)?;
    run_locked(command, args, stdout, stderr, &client, &journal)
}
fn run_locked(
    command: &str,
    args: &Args,
    stdout: &mut String,
    stderr: &mut String,
    c: &Client,
    journal: &Store,
) -> Result<i32> {
    if command == "status" {
        let id = &args.pos[1];
        let name = sdk::hex(&operation_nonce(&args.pos[0], id));
        let old: Option<Record> = journal.load(&name).map_err(journal_error)?;
        if old.as_ref().is_some_and(|v| {
            v.policy_id != args.pos[0]
                || v.request_id != *id
                || !same_retained_policy_config(&c.bundle.config, &v.config)
        }) {
            return Err(Error::config(
                "The journal does not bind this installed Stellar policy",
            ));
        }
        return poll(c, id, old, journal, &name, args.wait, stdout);
    }
    if args.flags.get("request-file").is_empty()
        || args
            .flags
            .0
            .keys()
            .any(|k| !matches!(k.as_str(), "request-file" | "request-id"))
    {
        return Err(Error::usage(
            "Stellar exec requires --request-file without scalar request flags",
        ));
    }
    let typed = read_typed_file(Path::new(args.flags.get("request-file")))?;
    let TypedInput::Execution(request) = typed.input else {
        return Err(Error::usage(
            "The Stellar profile accepts only execution-request input",
        ));
    };
    if request.expense.is_some() {
        return Err(Error::usage(
            "Expense facts are derived by the server, not supplied in the customer request",
        ));
    }
    let id = typed.request_id;
    if !args.flags.get("request-id").is_empty() && args.flags.get("request-id") != id {
        return Err(Error::usage("--request-id differs from the request file"));
    }
    if request.operation.nonce != operation_nonce(&args.pos[0], &id) {
        return Err(Error::usage(
            "The operation nonce does not bind this policy and request ID",
        ));
    }
    checked(sdk::operation_from_request(&c.bundle.config, &request))?;
    let name = sdk::hex(&request.operation.nonce);
    let old = journal.load::<Record>(&name).map_err(journal_error)?;
    let (mut record, resumed) = if let Some(old) = old {
        if old.policy_id != args.pos[0]
            || old.request_id != id
            || !same_retained_policy_config(&c.bundle.config, &old.config)
            || !same_execution_intent(&old.request, &request)
        {
            return Err(Error::config(
                "The original request ID already binds different input",
            ));
        }
        if old.agent_signature.is_some()
            || !matches!(&old.status, None | Some(OperationStatus::Prepared))
        {
            return poll(c, &id, Some(old), journal, &name, args.wait, stdout);
        }
        if !same_policy_config(&c.bundle.config, &old.config) {
            return Err(Error::uncertain(
                "The customer key changed. Read the original operation with status; do not authorize replacement bytes.",
            ));
        }
        let pure_intent = old.prepared.is_none() && old.status.is_none();
        let resumed = match c.status(&id, Some(&old)) {
            Ok(response)
                if response.operation.status == OperationStatus::Prepared
                    && response.signed_operation.is_none() =>
            {
                Some(response)
            }
            Ok(_) => return poll(c, &id, Some(old), journal, &name, args.wait, stdout),
            Err(error) if error.status == Some(404) && pure_intent => None,
            Err(error) => return Err(error),
        };
        (old, resumed)
    } else {
        (
            Record {
                policy_id: args.pos[0].clone(),
                request_id: id.clone(),
                config: c.bundle.config.clone(),
                request: request.clone(),
                prepared: None,
                agent_signature: None,
                status: None,
            },
            None,
        )
    };
    // Preserve the original intent. Only an explicit exec can continue unsigned preparation.
    journal.save(&name, &record).map_err(journal_error)?;
    *stderr += &format!("requestId={id}\n");
    crate::flush_stderr(stderr);
    let mut prepared = if let Some(response) = resumed {
        response
    } else {
        c.post(
            &c.bundle.endpoints.execute,
            json!({"policyId":args.pos[0],"requestId":id,"phase":"prepare","request":request}),
        )?
    };
    c.validate(&prepared, &id, Some(&record), false)?;
    if prepared.operation.status != OperationStatus::Prepared || prepared.signed_operation.is_some()
    {
        return Err(Error::uncertain(
            "Stellar prepare returned an unexpected lifecycle state",
        ));
    }
    // Retain observed bytes even when stale. They must never become a fresh all-None intent.
    record.prepared = Some(prepared.prepared.clone());
    record.status = Some(OperationStatus::Prepared);
    journal.save(&name, &record).map_err(journal_error)?;
    prepared.prepared = revalidate_for_signing(&prepared.prepared, clock()?)?;
    checked(sdk::validate_prepared(&prepared.prepared, clock()?))?;
    let signature = checked(sdk::sign(&prepared.prepared, &c.seed, clock()?))?;
    record.prepared = Some(prepared.prepared);
    record.agent_signature = Some(signature.clone());
    record.status = Some(OperationStatus::SubmissionUnknown);
    journal.save(&name, &record).map_err(journal_error)?;
    let response = c.post(
        &c.bundle.endpoints.execute,
        json!({"policyId":args.pos[0],"requestId":id,"phase":"submit","agentSignature":signature}),
    )?;
    c.validate(&response, &id, Some(&record), false)?;
    record.status = Some(response.operation.status.clone());
    journal.save(&name, &record).map_err(journal_error)?;
    if terminal(&response.operation.status) || args.wait.is_zero() {
        return render(&response, stdout);
    }
    poll(c, &id, Some(record), journal, &name, args.wait, stdout)
}
fn terminal(status: &OperationStatus) -> bool {
    matches!(status, OperationStatus::Confirmed | OperationStatus::Failed)
}
fn poll(
    c: &Client,
    id: &str,
    mut record: Option<Record>,
    journal: &Store,
    name: &str,
    wait: Duration,
    stdout: &mut String,
) -> Result<i32> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        let r = c.status(id, record.as_ref())?;
        let mut old = record.unwrap_or_else(|| Record {
            policy_id: c.bundle.policy_id.clone(),
            request_id: id.into(),
            config: c.bundle.config.clone(),
            request: r.prepared.request.clone(),
            prepared: Some(r.prepared.clone()),
            agent_signature: None,
            status: None,
        });
        if old.prepared.is_none() {
            old.prepared = Some(r.prepared.clone());
        }
        old.status = Some(r.operation.status.clone());
        journal.save(name, &old).map_err(journal_error)?;
        record = Some(old);
        if terminal(&r.operation.status) || std::time::Instant::now() >= deadline {
            return render(&r, stdout);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
fn render(r: &Response, stdout: &mut String) -> Result<i32> {
    *stdout += &format!(
        "{}\n",
        json!({"requestId":r.operation.id,"status":r.operation.status,"transactionHash":r.operation.transaction_hash})
    );
    Ok(match r.operation.status {
        OperationStatus::Confirmed => 0,
        OperationStatus::Failed => 20,
        OperationStatus::SubmissionUnknown => 5,
        OperationStatus::Prepared | OperationStatus::Submitted => 12,
    })
}

#[cfg(test)]
mod public_bundle_tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        let f: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/stellar-approval.json")).unwrap();
        let policy = &f["policy"];
        let config = WalletConfig {
 prefix_digest: None,
            network: "stellar:testnet".into(),
            policy_instance: [7; 32],
            wallet: f["intent"]["owner"].as_str().unwrap().into(),
            owner: f["intent"]["owner"].as_str().unwrap().into(),
            agent: sdk::account_for_seed(&[2; 32]),
            server: sdk::account_for_seed(&[3; 32]),
            route: WalletRoute::Native,
            policy_engine: None,
            asset: policy["mint"].as_str().unwrap().into(),
            asset_symbol: "XLM".into(),
            asset_issuer: None,
            policy_source: policy["code"].as_str().unwrap().into(),
            source_hash: sdk::unhex(policy["sourceHash"].as_str().unwrap()).unwrap(),
            module_digest: [5; 32],
            max_fee_stroops: 100,
        };
        json!({"version":1,"profile":sdk::PROFILE,"network":config.network,
            "policyId":policy["id"],"sourceHash":policy["sourceHash"],"irHash":policy["irHash"],
            "moduleDigest":sdk::hex(&config.module_digest),"wallet":config.wallet,
            "route":"native","config":config,
            "endpoints":{"execute":"/api/native/execute","status":"/api/native/execute/status"}})
    }
    #[test]
    fn public_inspection_binds_roles_and_source_without_loading_a_signer() {
        let value = fixture();
        let bundle: Bundle = decode_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        validate_bundle("existing-native-policy", &bundle).unwrap();
        let description = public_description(&bundle, false);
        assert!(description.get("policy").is_none());
        assert!(description.get("signingSeed").is_none());
        assert_eq!(description["agent"], sdk::account_for_seed(&[2; 32]));
        assert!(description["assetIssuer"].is_null());
        assert_eq!(
            public_description(&bundle, true)["policy"],
            value["config"]["policySource"]
        );
        let mut wrong: Bundle = decode_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        wrong.network = "stellar:mainnet".into();
        assert!(validate_bundle("existing-native-policy", &wrong).is_err());
        let mut issued: Bundle = decode_bytes(&serde_json::to_vec(&value).unwrap()).unwrap();
        issued.config.asset_symbol = "XLM".into();
        issued.config.asset_issuer = Some(sdk::account_for_seed(&[8; 32]));
        issued.config.asset = sdk::sac_address(
            &issued.config.network,
            &sdk::asset_xdr(&issued.config).unwrap(),
        )
        .unwrap();
        validate_bundle("existing-native-policy", &issued).unwrap();
        assert_eq!(
            public_description(&issued, false)["assetIssuer"],
            sdk::account_for_seed(&[8; 32])
        );
    }
    #[test]
    fn public_file_network_inference_refuses_private_fields_and_changed_identity() {
        let path = std::env::temp_dir().join(format!(
            "allowit-stellar-public-{}.json",
            std::process::id()
        ));
        let mut value = fixture();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            public_binding(&path).unwrap(),
            ("existing-native-policy".into(), "stellar:testnet".into())
        );
        value["signingSeed"] = json!("must-stay-local");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(public_binding(&path).is_err());
        value.as_object_mut().unwrap().remove("signingSeed");
        value["config"]["agent"] = value["config"]["owner"].clone();
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(public_binding(&path).is_err());
        value = fixture();
        value["config"]["sourceHash"] = json!(vec![0; 32]);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(public_binding(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod native_revalidation_tests {
    use super::*;
    use std::{io::Write, net::TcpListener};
    fn fixture() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/stellar-policy-wallet-revalidation-rpc.json"
        ))
        .unwrap()
    }
    #[test]
    fn local_rpc_revalidates_exact_gateway_bytes_before_signing_without_credentials() {
        let data = fixture();
        let p: PreparedOperation = serde_json::from_value(data["prepared"].clone()).unwrap();
        let calls = data["calls"].as_array().unwrap().clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for expected in calls {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    if bytes.ends_with(b"\r\n\r\n") {
                        break bytes.len();
                    }
                };
                let headers = std::str::from_utf8(&bytes).unwrap().to_ascii_lowercase();
                assert!(!headers.contains("authorization:"));
                assert!(!headers.contains("cookie:"));
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                bytes.resize(end + length, 0);
                stream.read_exact(&mut bytes[end..]).unwrap();
                let body = std::str::from_utf8(&bytes[end..]).unwrap();
                assert!(!body.contains("Bearer"));
                assert!(!body.contains("signing_seed"));
                assert!(!body.contains("ALLOWIT_TOKEN"));
                let request: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!(request["method"], expected["method"]);
                assert_eq!(request["params"], expected["params"]);
                assert_eq!(request["jsonrpc"], "2.0");
                let response = serde_json::to_vec(
                    &json!({"jsonrpc":"2.0","id":request["id"],"result":expected["result"]}),
                )
                .unwrap();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",response.len()).unwrap();
                stream.write_all(&response).unwrap();
            }
        });
        let checked = revalidate_with_rpc_url(&p, 86402, &url).unwrap();
        server.join().unwrap();
        assert_eq!(checked, p);
        assert_eq!(checked.transaction_xdr, p.transaction_xdr);
        assert_eq!(checked.transaction_hash, p.transaction_hash);
        assert!(checked.native_preflight.is_some());
        sdk::sign(&checked, &[2; 32], 86402).unwrap();
    }
    #[test]
    fn missing_rpc_stale_and_tampered_intents_fail_before_signing_but_classic_bypasses() {
        let data = fixture();
        let p: PreparedOperation = serde_json::from_value(data["prepared"].clone()).unwrap();
        assert!(revalidate_with_rpc_url(&p, 86402, "").is_err());
        assert!(revalidate_with_rpc_url(&p, 86700, "http://127.0.0.1:1").is_err());
        let mut changed = p.clone();
        changed.request.operation.nonce[0] ^= 1;
        assert!(revalidate_with_rpc_url(&changed, 86402, "http://127.0.0.1:1").is_err());
        let old: SignedOperation = serde_json::from_str(include_str!(
            "../../stellar-rust/tests/fixtures/canonical-main-9682-native-signed.json"
        ))
        .unwrap();
        assert_eq!(
            revalidate_with_rpc_url(&old.prepared, 86402, "").unwrap(),
            old.prepared
        );
    }
    #[test]
    fn retained_signed_status_validation_does_not_need_local_rpc_or_new_approval() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/stellar-native-revalidation-rpc.json"
        ))
        .unwrap();
        let signed: SignedOperation = serde_json::from_value(data["signed"].clone()).unwrap();
        assert!(signed.prepared.native_preflight.is_none());
        let config = signed.prepared.config.clone();
        let client = Client {
            http: reqwest::blocking::Client::new(),
            origin: "http://127.0.0.1:1".into(),
            bundle: Bundle {
                version: 1,
                profile: sdk::PROFILE.into(),
                network: config.network.clone(),
                policy_id: "policy-current".into(),
                source_hash: sdk::hex(&config.source_hash),
                ir_hash: String::new(),
                module_digest: sdk::hex(&config.module_digest),
                wallet: config.wallet.clone(),
                route: config.route.clone(),
                config,
                endpoints: Endpoints {
                    execute: "/api/native/execute".into(),
                    status: "/api/native/execute/status".into(),
                },
                signing_seed: None,
            },
            seed: Zeroizing::new([2; 32]),
        };
        let response = Response {
            prepared: signed.prepared.clone(),
            operation: Operation {
                id: "request-current".into(),
                status: OperationStatus::Submitted,
                transaction_hash: sdk::hex(&signed.transaction_hash),
            },
            signed_operation: Some(signed),
            receipt: None,
        };
        client
            .validate(&response, "request-current", None, true)
            .unwrap();
    }
}
