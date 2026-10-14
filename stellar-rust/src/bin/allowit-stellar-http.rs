//! Bounded, offline JSON interface for merchant verification. No signing or RPC.
use allowit_stellar::http_binding::{
    attestation_digest, inspect_payment_authorization, verify_http_payment, AttestationClaims,
    HttpRequest,
};
use allowit_stellar::{
    canonical_json, hex, ledger_keys, parse_vault_state, request_hash, sha256,
    validate_contract_entries, Config, PaymentRequest, Result, VaultState,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};

const MAX_INPUT_BYTES: usize = 8_388_608;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    CanonicalHash {
        value: Value,
    },
    RequestHash {
        request: PaymentRequest,
    },
    Inspect {
        credential: String,
    },
    Verify {
        config: Box<Config>,
        state: Box<VaultState>,
        request: PaymentRequest,
        actual: HttpRequest,
        credential: String,
        now: u64,
        #[serde(rename = "latestLedger")]
        latest_ledger: u32,
    },
    AttestationDigest {
        claims: AttestationClaims,
    },
    ParseState {
        #[serde(rename = "resultXdr")]
        result_xdr: String,
    },
    LedgerKeys {
        config: Config,
    },
    ValidateEntries {
        config: Config,
        entries: Vec<Value>,
    },
}

fn run(input: Input) -> Result<Value> {
    match input {
        Input::CanonicalHash { value } => Ok(json!(hex(&sha256(canonical_json(&value)?)))),
        Input::RequestHash { request } => Ok(json!(hex(&request_hash(&request)?))),
        Input::Inspect { credential } => {
            serde_json::to_value(inspect_payment_authorization(&credential)?)
                .map_err(|e| e.to_string())
        }
        Input::Verify {
            config,
            state,
            request,
            actual,
            credential,
            now,
            latest_ledger,
        } => serde_json::to_value(verify_http_payment(
            &config,
            &state,
            &request,
            &actual,
            &credential,
            now,
            latest_ledger,
        )?)
        .map_err(|e| e.to_string()),
        Input::AttestationDigest { claims } => Ok(json!(hex(&attestation_digest(&claims)?))),
        Input::ParseState { result_xdr } => {
            serde_json::to_value(parse_vault_state(&result_xdr)?).map_err(|e| e.to_string())
        }
        Input::LedgerKeys { config } => Ok(json!(ledger_keys(&config)?)),
        Input::ValidateEntries { config, entries } => {
            validate_contract_entries(&config, &entries)?;
            Ok(Value::Null)
        }
    }
}

fn execute() -> Result<Value> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("JSON input exceeds the 8 MiB limit".into());
    }
    let input = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    run(input)
}

fn main() {
    let response = match execute() {
        Ok(result) => json!({"ok":true,"result":result}),
        Err(error) => json!({"ok":false,"error":error}),
    };
    let mut stdout = std::io::stdout().lock();
    if serde_json::to_writer(&mut stdout, &response).is_err() || stdout.write_all(b"\n").is_err() {
        std::process::exit(1);
    }
}
