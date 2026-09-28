use allowit_contract_core::{
    Error, Mandate, Request, State, mandate_hash, prepare_execution, record_execution,
    validate_chain_artifact, validate_mandate,
};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{AccountInfo, next_account_info},
    clock::Clock,
    entrypoint::ProgramResult,
    program::invoke_signed,
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};
use spl_token::state::{Account as TokenAccount, Mint};

pub const STATE_BYTES: usize = 24_576;
pub const HEAD_BYTES: usize = 48;
pub const REQUIRED_HEAP_BYTES: u32 = 256 * 1024;
pub const REQUIRED_COMPUTE_UNITS: u32 = 1_400_000;
pub const MAX_SOLANA_ARTIFACT_BYTES: usize = allowit_contract_core::MAX_CHAIN_ARTIFACT_BYTES;
pub const MAX_SOLANA_IR_DEPTH: usize = allowit_contract_core::MAX_CHAIN_IR_DEPTH;
pub const MAX_SOLANA_IR_NODES: usize = allowit_contract_core::MAX_CHAIN_IR_NODES;
pub const MAX_RUNTIME_CONTEXT_BYTES: usize = 256;
const MAGIC: &[u8; 8] = b"ALLOWIT1";
pub const DEPLOYMENT_NETWORK: &str = match option_env!("ALLOWIT_SOLANA_NETWORK") {
    Some(v) => v,
    None => "devnet",
};
pub fn network_label() -> &'static str {
    match DEPLOYMENT_NETWORK {
        "mainnet" => "solana:mainnet",
        "testnet" => "solana:testnet",
        _ => "solana:devnet",
    }
}

#[cfg(all(target_os = "solana", feature = "custom-heap"))]
#[global_allocator]
static ALLOCATOR: solana_program::entrypoint::BumpAllocator =
    solana_program::entrypoint::BumpAllocator {
        start: solana_program::entrypoint::HEAP_START_ADDRESS as usize,
        len: REQUIRED_HEAP_BYTES as usize,
    };

pub fn required_compute_budget_instructions() -> [solana_program::instruction::Instruction; 2] {
    let program_id = solana_program::pubkey!("ComputeBudget111111111111111111111111111111");
    let mut heap = vec![1];
    heap.extend_from_slice(&REQUIRED_HEAP_BYTES.to_le_bytes());
    let mut compute = vec![2];
    compute.extend_from_slice(&REQUIRED_COMPUTE_UNITS.to_le_bytes());
    [
        solana_program::instruction::Instruction {
            program_id,
            accounts: vec![],
            data: heap,
        },
        solana_program::instruction::Instruction {
            program_id,
            accounts: vec![],
            data: compute,
        },
    ]
}

pub fn canonical_usdc() -> Option<Pubkey> {
    match DEPLOYMENT_NETWORK {
        "mainnet" => Some(solana_program::pubkey!(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        )),
        "devnet" => Some(solana_program::pubkey!(
            "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
        )),
        _ => None, // Circle has no canonical Solana Testnet USDC mint.
    }
}

/// Smaller target limits are checked before activation; the shared language's
/// larger headless limits do not imply that every policy fits a chain VM.
pub fn validate_target_artifact(mandate: &Mandate, bytes: &[u8]) -> Result<(), ProgramError> {
    validate_chain_artifact(mandate, bytes).map_err(fail)
}

pub fn head_address(program: &Pubkey, owner: &Pubkey, policy_id: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"allowit-head", owner.as_ref(), policy_id], program)
}

fn head(
    account: &AccountInfo,
    program: &Pubkey,
    mandate: &Mandate,
) -> Result<(u64, [u8; 32]), ProgramError> {
    let expected = head_address(
        program,
        &Pubkey::new_from_array(mandate.owner),
        &mandate.policy_id,
    )
    .0;
    if *account.key != expected || account.owner != program {
        return Err(fail(Error::InvalidAccount));
    }
    let data = account.try_borrow_data()?;
    if data.len() != HEAD_BYTES || &data[..8] != b"ALTHD001" {
        return Err(fail(Error::InvalidAccount));
    }
    Ok((
        u64::from_le_bytes(data[8..16].try_into().unwrap()),
        data[16..48].try_into().unwrap(),
    ))
}

fn write_head(account: &AccountInfo, revision: u64, active: &[u8; 32]) -> ProgramResult {
    if !account.is_writable {
        return Err(fail(Error::InvalidAccount));
    }
    let mut data = account.try_borrow_mut_data()?;
    if data.len() != HEAD_BYTES {
        return Err(fail(Error::InvalidAccount));
    }
    data[..8].copy_from_slice(b"ALTHD001");
    data[8..16].copy_from_slice(&revision.to_le_bytes());
    data[16..48].copy_from_slice(active);
    Ok(())
}

#[derive(Clone, Debug, BorshDeserialize, BorshSerialize)]
pub enum Instruction {
    /// Accounts: new program-owned state (signer, writable), owner (signer),
    /// Rent sysvar, logical-policy head. Owner creates/funds the state account first.
    Initialize { mandate: Mandate },
    /// Accounts: state (writable), owner (signer). Append only, before activation.
    Upload { offset: u32, bytes: Vec<u8> },
    /// Accounts: state (writable), owner (signer), compiler (signer), Clock, head(w).
    /// The compiler signs the exact mandate hash and artifact stored in it.
    Activate { expected_mandate_hash: String },
    /// Accounts: state(w), executor(signer), delegate PDA, source(w), destination(w),
    /// mint, classic SPL Token program, Clock, head, optional evidence signer.
    /// No arbitrary CPI is exposed.
    Execute { request: Request },
    /// Accounts: state(w), owner(signer). Irreversible for this mandate account.
    Revoke,
    /// Accounts: derived head(w), owner(signer,w), Rent, System Program.
    /// Create once per owner+logical policy before initializing its revisions.
    InitializeHead { policy_id: [u8; 32] },
}

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

fn fail(error: Error) -> ProgramError {
    ProgramError::Custom(error as u32)
}

fn authorized(account: &AccountInfo, expected: &[u8; 32]) -> ProgramResult {
    if !account.is_signer || account.key.to_bytes() != *expected {
        return Err(fail(Error::Unauthorized));
    }
    Ok(())
}

pub fn delegate_address(program: &Pubkey, state: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"allowit", state.as_ref()], program)
}

pub fn read_state(account: &AccountInfo, program: &Pubkey) -> Result<State, ProgramError> {
    if account.owner != program || !account.is_writable || account.executable {
        return Err(fail(Error::InvalidAccount));
    }
    let data = account.try_borrow_data()?;
    if data.len() != STATE_BYTES || &data[..8] != MAGIC {
        return Err(fail(Error::InvalidAccount));
    }
    let length = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    if length > data.len() - 12 {
        return Err(fail(Error::InvalidAccount));
    }
    borsh::from_slice(&data[12..12 + length]).map_err(|_| fail(Error::InvalidAccount))
}

fn write_state(account: &AccountInfo, state: &State) -> ProgramResult {
    let bytes = borsh::to_vec(state).map_err(|_| fail(Error::InvalidAccount))?;
    let mut data = account.try_borrow_mut_data()?;
    if data.len() != STATE_BYTES || bytes.len() > data.len() - 12 {
        return Err(ProgramError::AccountDataTooSmall);
    }
    data.fill(0);
    data[..8].copy_from_slice(MAGIC);
    data[8..12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    data[12..12 + bytes.len()].copy_from_slice(&bytes);
    Ok(())
}

fn clock(account: &AccountInfo) -> Result<u64, ProgramError> {
    let value = Clock::from_account_info(account)?;
    u64::try_from(value.unix_timestamp).map_err(|_| fail(Error::Expired))
}

pub fn process_instruction(
    program: &Pubkey,
    accounts: &[AccountInfo],
    data: &[u8],
) -> ProgramResult {
    if data.len() > 1024 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let instruction: Instruction =
        borsh::from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)?;
    let mut accounts = accounts.iter();
    let state_account = next_account_info(&mut accounts)?;
    let actor = next_account_info(&mut accounts)?;
    if let Instruction::InitializeHead { policy_id } = &instruction {
        if !actor.is_signer
            || !actor.is_writable
            || !state_account.is_writable
            || *policy_id == [0; 32]
        {
            return Err(fail(Error::Unauthorized));
        }
        let (expected, bump) = head_address(program, actor.key, policy_id);
        if *state_account.key != expected
            || state_account.owner != &solana_program::system_program::ID
            || !state_account.data_is_empty()
        {
            return Err(fail(Error::InvalidAccount));
        }
        let rent = Rent::from_account_info(next_account_info(&mut accounts)?)?;
        let system = next_account_info(&mut accounts)?;
        if system.key != &solana_program::system_program::ID || !system.executable {
            return Err(fail(Error::InvalidAccount));
        }
        let seeds: &[&[u8]] = &[b"allowit-head", actor.key.as_ref(), policy_id, &[bump]];
        // Allocate/assign permit a prefunded PDA; a one-lamport airdrop cannot
        // prevent initialization. Only the owner can create its logical policy.
        let needed = rent
            .minimum_balance(HEAD_BYTES)
            .saturating_sub(state_account.lamports());
        if needed > 0 {
            solana_program::program::invoke(
                &solana_program::system_instruction::transfer(actor.key, state_account.key, needed),
                &[actor.clone(), state_account.clone(), system.clone()],
            )?;
        }
        invoke_signed(
            &solana_program::system_instruction::allocate(state_account.key, HEAD_BYTES as u64),
            &[state_account.clone(), system.clone()],
            &[seeds],
        )?;
        invoke_signed(
            &solana_program::system_instruction::assign(state_account.key, program),
            &[state_account.clone(), system.clone()],
            &[seeds],
        )?;
        return write_head(state_account, 0, &[0; 32]);
    }
    if state_account.owner != program || !state_account.is_writable || state_account.executable {
        return Err(fail(Error::InvalidAccount));
    }
    match instruction {
        Instruction::Initialize { mandate } => {
            validate_mandate(&mandate).map_err(fail)?;
            authorized(actor, &mandate.owner)?;
            if mandate.network != network_label()
                || canonical_usdc().map(|key| key.to_bytes()) != Some(mandate.asset)
                || mandate.asset_decimals != 6
                || mandate.recipient_address
                    != Pubkey::new_from_array(mandate.recipient).to_string()
            {
                return Err(fail(Error::BindingMismatch));
            }
            if !state_account.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let rent_account = next_account_info(&mut accounts)?;
            let rent = Rent::from_account_info(rent_account)?;
            head(next_account_info(&mut accounts)?, program, &mandate)?;
            if !rent.is_exempt(state_account.lamports(), STATE_BYTES) {
                return Err(ProgramError::AccountNotRentExempt);
            }
            let data = state_account.try_borrow_data()?;
            if data.len() != STATE_BYTES || data.iter().any(|b| *b != 0) {
                return Err(fail(Error::AlreadyInitialized));
            }
            drop(data);
            write_state(
                state_account,
                &State {
                    mandate,
                    artifact: vec![],
                    active: false,
                    revoked: false,
                    spent_units: 0,
                    next_nonce: 0,
                },
            )
        }
        Instruction::Upload { offset, bytes } => {
            let mut state = read_state(state_account, program)?;
            authorized(actor, &state.mandate.owner)?;
            if state.active || state.revoked {
                return Err(fail(Error::Inactive));
            }
            if offset as usize != state.artifact.len()
                || bytes.is_empty()
                || bytes.len() > 700
                || state.artifact.len() + bytes.len() > MAX_SOLANA_ARTIFACT_BYTES
            {
                return Err(fail(Error::InvalidArtifact));
            }
            state.artifact.extend_from_slice(&bytes);
            write_state(state_account, &state)
        }
        Instruction::Activate {
            expected_mandate_hash,
        } => {
            let mut state = read_state(state_account, program)?;
            authorized(actor, &state.mandate.owner)?;
            let compiler = next_account_info(&mut accounts)?;
            authorized(compiler, &state.mandate.compiler)?;
            let now = clock(next_account_info(&mut accounts)?)?;
            let head_account = next_account_info(&mut accounts)?;
            let (latest, _) = head(head_account, program, &state.mandate)?;
            if latest.checked_add(1) != Some(state.mandate.revision) {
                return Err(fail(Error::BindingMismatch));
            }
            if state.active || state.revoked || now >= state.mandate.expires_at {
                return Err(fail(Error::Inactive));
            }
            if state.mandate.network != network_label()
                || mandate_hash(&state.mandate).map_err(fail)? != expected_mandate_hash
            {
                return Err(fail(Error::BindingMismatch));
            }
            validate_target_artifact(&state.mandate, &state.artifact)?;
            state.active = true;
            write_head(
                head_account,
                state.mandate.revision,
                &state_account.key.to_bytes(),
            )?;
            write_state(state_account, &state)
        }
        Instruction::Execute { request } => {
            if request.runtime_context.len() > MAX_RUNTIME_CONTEXT_BYTES {
                return Err(fail(Error::InvalidEvidence));
            }
            let mut state = read_state(state_account, program)?;
            authorized(actor, &state.mandate.executor)?;
            let authority = next_account_info(&mut accounts)?;
            let source = next_account_info(&mut accounts)?;
            let destination = next_account_info(&mut accounts)?;
            let mint_account = next_account_info(&mut accounts)?;
            let token_program = next_account_info(&mut accounts)?;
            let now = clock(next_account_info(&mut accounts)?)?;
            let (latest, active) =
                head(next_account_info(&mut accounts)?, program, &state.mandate)?;
            if latest != state.mandate.revision || active != state_account.key.to_bytes() {
                return Err(fail(Error::Inactive));
            }
            if state.mandate.network != network_label() {
                return Err(fail(Error::BindingMismatch));
            }
            if request.evidence.is_some() {
                let attester = state
                    .mandate
                    .evidence_authority
                    .as_ref()
                    .ok_or(fail(Error::EvidenceRequired))?;
                let evidence_signer = next_account_info(&mut accounts)?;
                // The signer authorizes this complete instruction, including
                // exact request hash, nonce, expiry and interval values.
                authorized(evidence_signer, &attester.key)?;
            }
            let (delegate, bump) = delegate_address(program, state_account.key);
            if authority.key != &delegate
                || token_program.key != &spl_token::ID
                || !token_program.executable
                || source.owner != &spl_token::ID
                || destination.owner != &spl_token::ID
                || mint_account.owner != &spl_token::ID
                || !source.is_writable
                || !destination.is_writable
                || source.key == destination.key
                || mint_account.key.to_bytes() != state.mandate.asset
            {
                return Err(fail(Error::InvalidAccount));
            }
            let source_state = TokenAccount::unpack(&source.try_borrow_data()?)?;
            let destination_state = TokenAccount::unpack(&destination.try_borrow_data()?)?;
            let mint = Mint::unpack(&mint_account.try_borrow_data()?)?;
            if mint.decimals != state.mandate.asset_decimals as u8
                || !mint.is_initialized
                || source_state.owner.to_bytes() != state.mandate.owner
                || source_state.mint != *mint_account.key
                || destination_state.mint != *mint_account.key
                || destination_state.owner.to_bytes() != state.mandate.recipient
                || source_state.delegate != COption::Some(delegate)
                || source_state.delegated_amount < request.amount_units
            {
                return Err(fail(Error::BindingMismatch));
            }
            prepare_execution(&state, &request, now).map_err(fail)?;
            let transfer = spl_token::instruction::transfer_checked(
                &spl_token::ID,
                source.key,
                mint_account.key,
                destination.key,
                &delegate,
                &[],
                request.amount_units,
                mint.decimals,
            )?;
            invoke_signed(
                &transfer,
                &[
                    source.clone(),
                    mint_account.clone(),
                    destination.clone(),
                    authority.clone(),
                    token_program.clone(),
                ],
                &[&[b"allowit", state_account.key.as_ref(), &[bump]]],
            )?;
            record_execution(&mut state, &request).map_err(fail)?;
            write_state(state_account, &state)
        }
        Instruction::Revoke => {
            let mut state = read_state(state_account, program)?;
            authorized(actor, &state.mandate.owner)?;
            state.revoked = true;
            state.active = false;
            write_state(state_account, &state)
        }
        Instruction::InitializeHead { .. } => Err(ProgramError::InvalidInstructionData),
    }
}
