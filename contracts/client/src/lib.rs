//! Host-only adapter. All artifact hashes, PDA derivation, Borsh instructions and
//! preflight decisions come from the existing compiler and rail libraries.
use allowit_contract_core::{
    Artifact, Evidence, EvidenceAuthority, Interval, Mandate, Request, State,
};
use allowit_solana::Instruction as RailInstruction;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    sysvar,
};
use std::str::FromStr;

type Result<T> = std::result::Result<T, String>;

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} must be a string"))
}
fn optional<'a>(value: &'a Value, name: &str, default: &'a str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or(default)
}
fn units(value: &Value, name: &str) -> Result<u64> {
    let raw = field(value, name)?;
    if raw.is_empty()
        || !raw.bytes().all(|b| b.is_ascii_digit())
        || (raw.len() > 1 && raw.starts_with('0'))
    {
        return Err(format!(
            "{name} must be a canonical unsigned decimal string"
        ));
    }
    raw.parse().map_err(|_| format!("{name} is outside u64"))
}
fn key(value: &Value, name: &str) -> Result<Pubkey> {
    Pubkey::from_str(field(value, name)?).map_err(|_| format!("{name} must be a base58 public key"))
}
fn id(raw: &str) -> Result<[u8; 32]> {
    if raw.len() != 64
        || !raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("policyId must be 32-byte lowercase hex".into());
    }
    let mut output = [0; 32];
    for (i, pair) in raw.as_bytes().chunks_exact(2).enumerate() {
        output[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    Ok(output)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn address(bytes: [u8; 32]) -> String {
    Pubkey::new_from_array(bytes).to_string()
}
fn core<T>(result: std::result::Result<T, allowit_contract_core::Error>) -> Result<T> {
    result.map_err(|error| format!("{error:?} ({})", error as u32))
}
pub fn instruction_dto(instruction: &Instruction) -> Value {
    json!({"programAddress":instruction.program_id.to_string(),
        "accounts":instruction.accounts.iter().map(|a| json!({"address":a.pubkey.to_string(),"isSigner":a.is_signer,"isWritable":a.is_writable})).collect::<Vec<_>>(),
        "dataBase64":STANDARD.encode(&instruction.data)})
}
fn rail(
    program: Pubkey,
    accounts: Vec<AccountMeta>,
    instruction: RailInstruction,
    label: String,
) -> Result<Value> {
    let data = borsh::to_vec(&instruction).map_err(|e| e.to_string())?;
    if data.len() > 1024 {
        return Err(format!(
            "{label} exceeds the program's 1024-byte instruction limit"
        ));
    }
    let mut value = instruction_dto(&Instruction {
        program_id: program,
        accounts,
        data,
    });
    value["label"] = label.into();
    Ok(value)
}
fn compiled(value: &Value) -> Result<(Artifact, Vec<u8>)> {
    let artifact = allowit_contract_core::compile_artifact(
        field(value, "source")?,
        field(value, "originalIntent")?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let bytes = serde_json::to_vec(&artifact).map_err(|e| e.to_string())?;
    Ok((artifact, bytes))
}
fn mandate(value: &Value, artifact: &Artifact, bytes: &[u8]) -> Result<Mandate> {
    let evidence_authority = match value.get("evidenceAuthority").filter(|v| !v.is_null()) {
        Some(authority) => Some(EvidenceAuthority {
            key: key(authority, "address")?.to_bytes(),
            key_id: field(authority, "keyId")?.into(),
            version: field(authority, "version")?.into(),
        }),
        None => None,
    };
    let recipient = key(value, "recipient")?;
    Ok(Mandate {
        policy_id: id(field(value, "policyId")?)?,
        owner: key(value, "owner")?.to_bytes(),
        executor: key(value, "executor")?.to_bytes(),
        compiler: key(value, "compiler")?.to_bytes(),
        compiler_key_id: optional(value, "compilerKeyId", "demo-compiler").into(),
        compiler_version: artifact.compiler_version.clone(),
        evidence_authority,
        registry_version: artifact.registry_version.clone(),
        core_version: artifact.core_version.clone(),
        network: allowit_solana::network_label().into(),
        asset: allowit_solana::canonical_usdc()
            .ok_or("unsupported deployment network")?
            .to_bytes(),
        asset_decimals: 6,
        recipient: recipient.to_bytes(),
        recipient_address: recipient.to_string(),
        action: optional(value, "action", "transfer").into(),
        merchant: optional(value, "merchant", "demo").into(),
        revision: units(value, "revision")?,
        expires_at: units(value, "expiresAt")?,
        allocation_units: units(value, "allocationUnits")?,
        source_hash: artifact.source_hash.clone(),
        ir_hash: artifact.ir_hash.clone(),
        artifact_hash: allowit_sdk::digest(bytes),
    })
}
fn authority_dto(authority: &EvidenceAuthority) -> Value {
    json!({"address":address(authority.key),"keyId":authority.key_id,"version":authority.version})
}
pub fn mandate_dto(m: &Mandate) -> Value {
    json!({"policyId":hex(&m.policy_id),"owner":address(m.owner),"executor":address(m.executor),"compiler":address(m.compiler),
        "compilerKeyId":m.compiler_key_id,"compilerVersion":m.compiler_version,"evidenceAuthority":m.evidence_authority.as_ref().map(authority_dto),
        "registryVersion":m.registry_version,"coreVersion":m.core_version,"network":m.network,"asset":address(m.asset),"assetDecimals":m.asset_decimals,
        "recipient":address(m.recipient),"recipientAddress":m.recipient_address,"action":m.action,"merchant":m.merchant,
        "revision":m.revision.to_string(),"expiresAt":m.expires_at.to_string(),"allocationUnits":m.allocation_units.to_string(),
        "sourceHash":m.source_hash,"irHash":m.ir_hash,"artifactHash":m.artifact_hash})
}
pub fn state_dto(state: &State) -> Value {
    json!({"mandate":mandate_dto(&state.mandate),"active":state.active,"revoked":state.revoked,
        "spentUnits":state.spent_units.to_string(),"nextNonce":state.next_nonce.to_string(),
        "remainingUnits":state.mandate.allocation_units.saturating_sub(state.spent_units).to_string(),"artifactBytes":state.artifact.len()})
}
pub fn decode_state(raw: &[u8]) -> Result<State> {
    if raw.len() != allowit_solana::STATE_BYTES || &raw[..8] != b"ALLOWIT1" {
        return Err("invalid state account size or magic".into());
    }
    let length = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    if length > raw.len() - 12 {
        return Err("invalid state length".into());
    }
    borsh::from_slice(&raw[12..12 + length]).map_err(|e| format!("invalid state: {e}"))
}
fn decode_input(value: &Value) -> Result<State> {
    decode_state(
        &STANDARD
            .decode(field(value, "stateBase64")?)
            .map_err(|e| e.to_string())?,
    )
}
fn compile_response(artifact: &Artifact, bytes: &[u8]) -> Value {
    json!({"sourceHash":artifact.source_hash,"irHash":artifact.ir_hash,"artifactHash":allowit_sdk::digest(bytes),
        "registryVersion":artifact.registry_version,"coreVersion":artifact.core_version,"compilerVersion":artifact.compiler_version,
        "compiledIR":artifact.ir,"artifactBase64":STANDARD.encode(bytes),"artifactBytes":bytes.len()})
}
fn compile_only(value: &Value) -> Result<Value> {
    let (artifact, bytes) = compiled(value)?;
    // Validation uses a valid placeholder binding; activation validates actual keys.
    let placeholder = json!({"policyId":"01".repeat(32),"owner":address([1;32]),"executor":address([2;32]),"compiler":address([3;32]),
        "recipient":address([4;32]),"revision":"1","expiresAt":"1","allocationUnits":"1"});
    core(allowit_contract_core::validate_chain_artifact(
        &mandate(&placeholder, &artifact, &bytes)?,
        &bytes,
    ))?;
    Ok(compile_response(&artifact, &bytes))
}
fn activation(value: &Value) -> Result<Value> {
    let program = key(value, "programId")?;
    let state = key(value, "stateAddress")?;
    let owner = key(value, "owner")?;
    let compiler = key(value, "compiler")?;
    let (artifact, bytes) = compiled(value)?;
    let mandate = mandate(value, &artifact, &bytes)?;
    core(allowit_contract_core::validate_chain_artifact(
        &mandate, &bytes,
    ))?;
    let hash = core(allowit_contract_core::mandate_hash(&mandate))?;
    let head = allowit_solana::head_address(&program, &owner, &mandate.policy_id).0;
    let delegate = allowit_solana::delegate_address(&program, &state).0;
    let mut instructions = vec![
        rail(
            program,
            vec![
                AccountMeta::new(head, false),
                AccountMeta::new(owner, true),
                AccountMeta::new_readonly(sysvar::rent::ID, false),
                AccountMeta::new_readonly(solana_program::system_program::ID, false),
            ],
            RailInstruction::InitializeHead {
                policy_id: mandate.policy_id,
            },
            "initialize-head".into(),
        )?,
        rail(
            program,
            vec![
                AccountMeta::new(state, true),
                AccountMeta::new_readonly(owner, true),
                AccountMeta::new_readonly(sysvar::rent::ID, false),
                AccountMeta::new_readonly(head, false),
            ],
            RailInstruction::Initialize {
                mandate: mandate.clone(),
            },
            "initialize-state".into(),
        )?,
    ];
    let chunk = value
        .get("uploadChunkBytes")
        .and_then(Value::as_u64)
        .unwrap_or(480) as usize;
    if !(1..=700).contains(&chunk) {
        return Err("uploadChunkBytes must be 1..700".into());
    }
    for (index, part) in bytes.chunks(chunk).enumerate() {
        instructions.push(rail(
            program,
            vec![
                AccountMeta::new(state, false),
                AccountMeta::new_readonly(owner, true),
            ],
            RailInstruction::Upload {
                offset: (index * chunk) as u32,
                bytes: part.to_vec(),
            },
            format!("upload-{index}"),
        )?);
    }
    instructions.push(rail(
        program,
        vec![
            AccountMeta::new(state, false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new_readonly(compiler, true),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new(head, false),
        ],
        RailInstruction::Activate {
            expected_mandate_hash: hash.clone(),
        },
        "activate".into(),
    )?);
    Ok(
        json!({"metadata":compile_response(&artifact,&bytes),"mandate":mandate_dto(&mandate),"mandateHash":hash,
        "stateAddress":state.to_string(),"headAddress":head.to_string(),"delegateAddress":delegate.to_string(),"stateBytes":allowit_solana::STATE_BYTES,
        "instructions":instructions,"computeBudgetInstructions":allowit_solana::required_compute_budget_instructions().iter().map(instruction_dto).collect::<Vec<_>>()}),
    )
}
fn execute(value: &Value) -> Result<Value> {
    let state = decode_input(value)?;
    let m = &state.mandate;
    let program = key(value, "programId")?;
    let state_key = key(value, "stateAddress")?;
    let runtime_context = value.get("context").cloned().unwrap_or_else(|| json!({}));
    if !runtime_context.is_object() {
        return Err("context must be a JSON object".into());
    }
    let runtime_context = serde_json::to_string(&runtime_context).map_err(|e| e.to_string())?;
    if runtime_context.len() > allowit_solana::MAX_RUNTIME_CONTEXT_BYTES {
        return Err("canonical context exceeds 256 bytes".into());
    }
    let mut request = Request {
        nonce: units(value, "nonce")?,
        revision: m.revision,
        amount_units: units(value, "amountUnits")?,
        asset: m.asset,
        recipient: m.recipient,
        network: m.network.clone(),
        action: m.action.clone(),
        merchant: m.merchant.clone(),
        source_hash: m.source_hash.clone(),
        ir_hash: m.ir_hash.clone(),
        evidence: None,
        runtime_context,
    };
    let bound_hash = core(allowit_contract_core::request_hash(m, &request))?;
    if let Some(input) = value.get("evidence").filter(|v| !v.is_null()) {
        let authority = m
            .evidence_authority
            .as_ref()
            .ok_or("mandate has no evidence authority")?;
        let intervals = input
            .get("intervals")
            .and_then(Value::as_array)
            .ok_or("evidence.intervals must be an array")?
            .iter()
            .map(|interval| {
                Ok(Interval {
                    name: field(interval, "name")?.into(),
                    lower_bps: units(interval, "lowerBps")?,
                    upper_bps: units(interval, "upperBps")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        request.evidence = Some(Evidence {
            request_hash: bound_hash.clone(),
            key_id: authority.key_id.clone(),
            version: authority.version.clone(),
            issued_at: units(input, "issuedAt")?,
            expires_at: units(input, "expiresAt")?,
            intervals,
        });
    }
    let decision = match allowit_contract_core::prepare_execution(
        &state,
        &request,
        units(value, "now")?,
    ) {
        Ok(decision) => json!({"allowed":true,"decision":decision}),
        Err(error) => {
            json!({"allowed":false,"error":format!("{error:?}"),"customError":error as u32,
            "status":if error==allowit_contract_core::Error::UserInputRequired {"awaiting_input"} else {"denied"}})
        }
    };
    let executor = Pubkey::new_from_array(m.executor);
    let delegate = allowit_solana::delegate_address(&program, &state_key).0;
    let head =
        allowit_solana::head_address(&program, &Pubkey::new_from_array(m.owner), &m.policy_id).0;
    let mut accounts = vec![
        AccountMeta::new(state_key, false),
        AccountMeta::new_readonly(executor, true),
        AccountMeta::new_readonly(delegate, false),
        AccountMeta::new(key(value, "sourceTokenAccount")?, false),
        AccountMeta::new(key(value, "destinationTokenAccount")?, false),
        AccountMeta::new_readonly(Pubkey::new_from_array(m.asset), false),
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(sysvar::clock::ID, false),
        AccountMeta::new_readonly(head, false),
    ];
    if request.evidence.is_some() {
        accounts.push(AccountMeta::new_readonly(
            Pubkey::new_from_array(m.evidence_authority.as_ref().unwrap().key),
            true,
        ));
    }
    // Rejected preflight still produces authoritative bytes for deliberate chain tests.
    Ok(
        json!({"requestHash":bound_hash,"nonce":request.nonce.to_string(),"revision":request.revision.to_string(),"contextHash":allowit_sdk::digest(request.runtime_context.as_bytes()),"runtimeContext":request.runtime_context,
        "preflight":decision,"instruction":rail(program,accounts,RailInstruction::Execute{request},"execute".into())?,
        "computeBudgetInstructions":allowit_solana::required_compute_budget_instructions().iter().map(instruction_dto).collect::<Vec<_>>()}),
    )
}
/// Inspect exact concrete PaySH request bytes without signing or submitting them.
fn inspect_paysh_request(value: &Value) -> Result<Value> {
    let bytes = STANDARD
        .decode(field(value, "requestBase64")?)
        .map_err(|e| e.to_string())?;
    let request: allowit_paysh_interface::Request =
        borsh::from_slice(&bytes).map_err(|e| format!("Invalid PaySH request: {e}"))?;
    let action = match request.action {
        allowit_paysh_interface::Action::PayUsdc { amount } if amount > 0 => {
            json!({"operation":"paysh::pay_usdc", "amountUnits":amount.to_string()})
        }
        allowit_paysh_interface::Action::SwapSolToUsdc {
            amount_in_lamports,
            min_out_usdc,
            sqrt_price_limit,
            tick_arrays,
        } if amount_in_lamports > 0 && min_out_usdc > 0 && sqrt_price_limit > 0 => {
            json!({"operation":"paysh::swap_sol_to_usdc", "amountInLamports":amount_in_lamports.to_string(),
                "minOutUsdc":min_out_usdc.to_string(), "sqrtPriceLimit":sqrt_price_limit.to_string(),
                "tickArrays":tick_arrays.map(address)})
        }
        _ => return Err("PaySH action amounts and price limits must be positive".into()),
    };
    if request.expires_slot <= request.signing_slot
        || request.expires_timestamp <= request.signing_timestamp
    {
        return Err("PaySH expiry must follow both signing clocks".into());
    }
    Ok(
        json!({"action":action, "requestHash":allowit_sdk::digest(&bytes),
        "signedMessageBase64":STANDARD.encode(request.signed_message()),
        "operationId":hex(&request.operation_id), "nonce":hex(&request.nonce),
        "challengeHash":hex(&request.challenge_hash), "evidenceHash":hex(&request.evidence_hash),
        "policy":address(request.policy), "owner":address(request.owner), "program":address(request.program),
        "network":address(request.network), "moduleDigest":hex(&request.module_digest),
        "expiresSlot":request.expires_slot.to_string(), "expiresTimestamp":request.expires_timestamp.to_string(),
        "requestBytes":bytes.len(), "executed":false}),
    )
}

/// Explicit namespaces select focused codec operations. Legacy `op` remains readable.
fn operation(value: &Value) -> Result<&str> {
    if let Some(name) = value.get("operation") {
        let name = name.as_str().ok_or("operation must be a string")?;
        if value.get("op").is_some() {
            return Err("Use operation or op, not both".into());
        }
        return match name {
            "allowit::compile_policy" => Ok("compile"),
            "jev::preference_key" => Ok("semantic-key"),
            "solana::prepare_activation" => Ok("activation"),
            "solana::prepare_execution" => Ok("execute"),
            "solana::decode_state" => Ok("decode-state"),
            "solana::prepare_revoke" => Ok("revoke"),
            "solana::prepare_state" => Ok("create-state"),
            "solana::prepare_delegate" => Ok("approve"),
            "solana::derive_addresses" => Ok("addresses"),
            "paysh::inspect_request" => Ok("inspect-paysh"),
            _ => Err("Unknown registered codec operation".into()),
        };
    }
    field(value, "op")
}

pub fn dispatch(value: &Value) -> Result<Value> {
    let mut output = match operation(value)? {
        "compile" => compile_only(value)?,
        "inspect-paysh" => inspect_paysh_request(value)?,
        "activation" => activation(value)?,
        "execute" => execute(value)?,
        "decode-state" => state_dto(&decode_input(value)?),
        "revoke" => {
            json!({"instruction":rail(key(value,"programId")?,vec![AccountMeta::new(key(value,"stateAddress")?,false),AccountMeta::new_readonly(key(value,"owner")?,true)],RailInstruction::Revoke,"revoke".into())?,
                "computeBudgetInstructions":allowit_solana::required_compute_budget_instructions().iter().map(instruction_dto).collect::<Vec<_>>()})
        }
        "create-state" => {
            json!({"instruction":instruction_dto(&solana_program::system_instruction::create_account(&key(value,"owner")?,&key(value,"stateAddress")?,units(value,"lamports")?,allowit_solana::STATE_BYTES as u64,&key(value,"programId")?))})
        }
        "approve" => {
            json!({"instruction":instruction_dto(&spl_token::instruction::approve_checked(&spl_token::ID,&key(value,"sourceTokenAccount")?,&allowit_solana::canonical_usdc().ok_or("unsupported network")?,&key(value,"delegateAddress")?,&key(value,"owner")?,&[],units(value,"allocationUnits")?,6).map_err(|e|e.to_string())?)})
        }
        "semantic-key" => {
            json!({"key":allowit_sdk::semantic_evidence_key(field(value,"question")?)})
        }
        "addresses" => {
            let program = key(value, "programId")?;
            let owner = key(value, "owner")?;
            let state = key(value, "stateAddress")?;
            json!({"headAddress":allowit_solana::head_address(&program,&owner,&id(field(value,"policyId")?)?).0.to_string(),"delegateAddress":allowit_solana::delegate_address(&program,&state).0.to_string(),"mint":allowit_solana::canonical_usdc().ok_or("unsupported network")?.to_string(),"network":allowit_solana::network_label()})
        }
        _ => return Err("unknown op".into()),
    };
    output["ok"] = true.into();
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"op":"activation","source":"pub async fn exec(ctx: &Context) -> PolicyResult { allowit::set_cap(ctx, \"5\", \"USDC\")?; Ok(()) }","originalIntent":"Only small USDC transfers",
            "policyId":"01".repeat(32),"programId":address([9;32]),"stateAddress":address([8;32]),"owner":address([1;32]),"executor":address([2;32]),"compiler":address([3;32]),"recipient":address([4;32]),
            "revision":"1","expiresAt":"2000","allocationUnits":"9007199254740993"})
    }
    #[test]
    fn instructions_reuse_rail_borsh_and_preserve_large_integers() {
        let value = dispatch(&fixture()).unwrap();
        assert_eq!(value["mandate"]["allocationUnits"], "9007199254740993");
        let list = value["instructions"].as_array().unwrap();
        let first = STANDARD
            .decode(list[0]["dataBase64"].as_str().unwrap())
            .unwrap();
        assert!(matches!(
            borsh::from_slice::<RailInstruction>(&first).unwrap(),
            RailInstruction::InitializeHead { .. }
        ));
        let second = STANDARD
            .decode(list[1]["dataBase64"].as_str().unwrap())
            .unwrap();
        let RailInstruction::Initialize { mandate } = borsh::from_slice(&second).unwrap() else {
            panic!("wrong instruction")
        };
        assert_eq!(mandate.allocation_units, 9_007_199_254_740_993);
        assert_eq!(
            core(allowit_contract_core::mandate_hash(&mandate)).unwrap(),
            value["mandateHash"]
        );
        let uploaded: Vec<u8> = list[2..list.len() - 1]
            .iter()
            .flat_map(|item| {
                let RailInstruction::Upload { bytes, .. } = borsh::from_slice(
                    &STANDARD
                        .decode(item["dataBase64"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap() else {
                    panic!("wrong upload")
                };
                bytes
            })
            .collect();
        core(allowit_contract_core::validate_chain_artifact(
            &mandate, &uploaded,
        ))
        .unwrap();
    }
    #[test]
    fn corrupt_state_and_non_decimal_u64_are_rejected() {
        assert!(decode_state(&vec![0; allowit_solana::STATE_BYTES]).is_err());
        let mut input = fixture();
        input["revision"] = json!(1);
        assert!(dispatch(&input).unwrap_err().contains("revision"));
        input["revision"] = json!("01");
        assert!(dispatch(&input).is_err());
    }
    #[test]
    fn revoke_requests_required_heap_and_preserves_owner_instruction() {
        let output = dispatch(&json!({"op":"revoke","programId":address([9;32]),
            "stateAddress":address([8;32]),"owner":address([1;32])}))
        .unwrap();
        let budgets = output["computeBudgetInstructions"].as_array().unwrap();
        assert_eq!(budgets.len(), 2);
        for (dto, tag, required) in [
            (&budgets[0], 1, allowit_solana::REQUIRED_HEAP_BYTES),
            (&budgets[1], 2, allowit_solana::REQUIRED_COMPUTE_UNITS),
        ] {
            assert_eq!(
                dto["programAddress"],
                "ComputeBudget111111111111111111111111111111"
            );
            assert!(dto["accounts"].as_array().unwrap().is_empty());
            let bytes = STANDARD
                .decode(dto["dataBase64"].as_str().unwrap())
                .unwrap();
            assert_eq!(bytes.len(), 5);
            assert_eq!(bytes[0], tag);
            assert_eq!(u32::from_le_bytes(bytes[1..].try_into().unwrap()), required);
        }
        assert_eq!(allowit_solana::REQUIRED_HEAP_BYTES, 256 * 1024);
        let bytes = STANDARD
            .decode(output["instruction"]["dataBase64"].as_str().unwrap())
            .unwrap();
        assert!(matches!(
            borsh::from_slice::<RailInstruction>(&bytes).unwrap(),
            RailInstruction::Revoke
        ));
        assert_eq!(output["instruction"]["accounts"][0]["isWritable"], true);
        assert_eq!(
            output["instruction"]["accounts"][1]["address"],
            address([1; 32])
        );
        assert_eq!(output["instruction"]["accounts"][1]["isSigner"], true);
    }
    #[test]
    fn reached_user_input_is_not_overridden_and_replay_is_bound() {
        let mut input = fixture();
        input["source"] = json!(
            "pub async fn exec(ctx: &Context) -> PolicyResult { require_user_input(ctx, \"Confirm transfer?\").await?; Ok(()) }"
        );
        let (artifact, bytes) = compiled(&input).unwrap();
        let m = mandate(&input, &artifact, &bytes).unwrap();
        let state = State {
            mandate: m,
            artifact: bytes,
            active: true,
            revoked: false,
            spent_units: 0,
            next_nonce: 7,
        };
        let serialized = borsh::to_vec(&state).unwrap();
        let mut raw = vec![0; allowit_solana::STATE_BYTES];
        raw[..8].copy_from_slice(b"ALLOWIT1");
        raw[8..12].copy_from_slice(&(serialized.len() as u32).to_le_bytes());
        raw[12..12 + serialized.len()].copy_from_slice(&serialized);
        let mut request = json!({"op":"execute","programId":address([9;32]),"stateAddress":address([8;32]),"stateBase64":STANDARD.encode(raw),"nonce":"7","amountUnits":"1","now":"1000","context":{},"sourceTokenAccount":address([5;32]),"destinationTokenAccount":address([6;32])});
        let result = dispatch(&request).unwrap();
        assert_eq!(result["preflight"]["error"], "UserInputRequired");
        assert!(result.get("instruction").is_some());
        request["nonce"] = json!("6");
        let replay = dispatch(&request).unwrap();
        assert_eq!(replay["preflight"]["error"], "Replay");
        assert_ne!(result["requestHash"], replay["requestHash"]);
    }
}
