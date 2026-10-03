//! Devnet-only fixed hard-limit wallet. This version does not execute arbitrary
//! policy IR: owner and compiler approve the explicit limits and source hashes.
//! The legacy IR/allowance adapter is a separate instruction namespace.
use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo,
    clock::Clock,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction, system_program,
    sysvar::Sysvar,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

pub const PREFIX: u8 = 0xa1;
pub const STATE_BYTES: usize = 305;
pub const CHARGE_BYTES: usize = 120;
pub const ATA_PROGRAM: Pubkey =
    solana_program::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const MAGIC: &[u8; 8] = b"ALVLT001";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Terms {
    pub policy_id: [u8; 32],
    pub executor: [u8; 32],
    pub compiler: [u8; 32],
    pub recipient: [u8; 32],
    pub service_hash: [u8; 32],
    pub source_hash: [u8; 32],
    pub ir_hash: [u8; 32],
    pub allocation: u64,
    pub per_call: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct State {
    pub policy_id: [u8; 32],
    pub owner: [u8; 32],
    pub executor: [u8; 32],
    pub compiler: [u8; 32],
    pub recipient: [u8; 32],
    pub service_hash: [u8; 32],
    pub source_hash: [u8; 32],
    pub ir_hash: [u8; 32],
    pub allocation: u64,
    pub per_call: u64,
    pub expires_at: u64,
    pub spent: u64,
    pub nonce: u64,
    pub revoked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    Activate(Terms),
    Execute {
        amount: u64,
        nonce: u64,
        challenge_hash: [u8; 32],
        request_hash: [u8; 32],
        expires_at: u64,
    },
    Revoke,
    WithdrawRemaining,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Error {
    InvalidAccount = 2000,
    Unauthorized,
    InvalidTerms,
    Inactive,
    Expired,
    Limit,
    Replay,
    InvalidNetwork,
}
fn err(e: Error) -> ProgramError {
    ProgramError::Custom(e as u32)
}
fn check(ok: bool, e: Error) -> ProgramResult {
    if ok { Ok(()) } else { Err(err(e)) }
}
pub fn address(program: &Pubkey, owner: &Pubkey, policy_id: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"allowit-vault", owner.as_ref(), policy_id], program)
}
pub fn charge_address(program: &Pubkey, vault: &Pubkey, challenge: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"allowit-charge", vault.as_ref(), challenge], program)
}
pub fn ata(owner: &Pubkey, mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), spl_token::ID.as_ref(), mint.as_ref()],
        &ATA_PROGRAM,
    )
    .0
}
pub fn encode(ix: &Instruction) -> Vec<u8> {
    let mut data = vec![PREFIX];
    data.extend(borsh::to_vec(ix).expect("fixed vault instruction"));
    data
}
pub fn read_state(info: &AccountInfo, program: &Pubkey) -> Result<State, ProgramError> {
    check(
        info.owner == program && !info.executable,
        Error::InvalidAccount,
    )?;
    let data = info.try_borrow_data()?;
    check(
        data.len() == STATE_BYTES && &data[..8] == MAGIC,
        Error::InvalidAccount,
    )?;
    let state: State = borsh::from_slice(&data[8..]).map_err(|_| err(Error::InvalidAccount))?;
    check(
        address(
            program,
            &Pubkey::new_from_array(state.owner),
            &state.policy_id,
        )
        .0 == *info.key,
        Error::InvalidAccount,
    )?;
    Ok(state)
}
fn write_state(info: &AccountInfo, state: &State) -> ProgramResult {
    check(info.is_writable, Error::InvalidAccount)?;
    let mut data = info.try_borrow_mut_data()?;
    check(data.len() == STATE_BYTES, Error::InvalidAccount)?;
    data[..8].copy_from_slice(MAGIC);
    data[8..].copy_from_slice(&borsh::to_vec(state).map_err(|_| err(Error::InvalidAccount))?);
    Ok(())
}
fn signer(info: &AccountInfo, key: &[u8; 32]) -> ProgramResult {
    check(
        info.is_signer && info.key.to_bytes() == *key,
        Error::Unauthorized,
    )
}
fn mint(info: &AccountInfo) -> Result<Pubkey, ProgramError> {
    let expected = crate::canonical_usdc().ok_or(err(Error::InvalidNetwork))?;
    check(
        *info.key == expected && *info.owner == spl_token::ID && !info.executable,
        Error::InvalidAccount,
    )?;
    let m = Mint::unpack(&info.try_borrow_data()?)?;
    check(m.is_initialized && m.decimals == 6, Error::InvalidAccount)?;
    Ok(expected)
}
fn token(
    info: &AccountInfo,
    owner: &Pubkey,
    mint: &Pubkey,
    writable: bool,
) -> Result<TokenAccount, ProgramError> {
    check(
        *info.owner == spl_token::ID
            && !info.executable
            && (!writable || info.is_writable)
            && *info.key == ata(owner, mint),
        Error::InvalidAccount,
    )?;
    let t = TokenAccount::unpack(&info.try_borrow_data()?)?;
    check(
        t.owner == *owner
            && t.mint == *mint
            && t.state == AccountState::Initialized
            && t.is_native == COption::None,
        Error::InvalidAccount,
    )?;
    Ok(t)
}
fn token_program(info: &AccountInfo) -> ProgramResult {
    check(
        *info.key == spl_token::ID && info.executable,
        Error::InvalidAccount,
    )
}
fn create<'a>(
    program: &Pubkey,
    target: &AccountInfo<'a>,
    payer: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    rent: &AccountInfo,
    bytes: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    check(
        target.is_writable
            && target.owner == &system_program::ID
            && target.data_is_empty()
            && !target.executable
            && payer.is_signer
            && payer.is_writable
            && *system.key == system_program::ID
            && system.executable,
        Error::InvalidAccount,
    )?;
    let minimum = Rent::from_account_info(rent)?.minimum_balance(bytes).max(1);
    let needed = minimum.saturating_sub(target.lamports());
    if needed > 0 {
        invoke(
            &system_instruction::transfer(payer.key, target.key, needed),
            &[payer.clone(), target.clone(), system.clone()],
        )?;
    }
    // Allocate/assign accepts prefunded PDAs without making address squatting a DoS.
    invoke_signed(
        &system_instruction::allocate(target.key, bytes as u64),
        &[target.clone(), system.clone()],
        &[seeds],
    )?;
    invoke_signed(
        &system_instruction::assign(target.key, program),
        &[target.clone(), system.clone()],
        &[seeds],
    )
}
fn now(clock: &AccountInfo) -> Result<u64, ProgramError> {
    u64::try_from(Clock::from_account_info(clock)?.unix_timestamp).map_err(|_| err(Error::Expired))
}
pub fn check_payment(
    state: &State,
    amount: u64,
    nonce: u64,
    expiry: u64,
    clock: u64,
) -> ProgramResult {
    check(!state.revoked, Error::Inactive)?;
    check(clock < state.expires_at && clock < expiry, Error::Expired)?;
    check(state.nonce.checked_add(1) == Some(nonce), Error::Replay)?;
    check(
        amount > 0
            && amount <= state.per_call
            && state
                .spent
                .checked_add(amount)
                .is_some_and(|n| n <= state.allocation),
        Error::Limit,
    )
}
fn transfer<'a>(
    from: &AccountInfo<'a>,
    to: &AccountInfo<'a>,
    mint: &AccountInfo<'a>,
    authority: &AccountInfo<'a>,
    token: &AccountInfo<'a>,
    amount: u64,
    seeds: &[&[u8]],
) -> ProgramResult {
    let ix = spl_token::instruction::transfer_checked(
        &spl_token::ID,
        from.key,
        mint.key,
        to.key,
        authority.key,
        &[],
        amount,
        6,
    )?;
    let infos = [
        from.clone(),
        mint.clone(),
        to.clone(),
        authority.clone(),
        token.clone(),
    ];
    if seeds.is_empty() {
        invoke(&ix, &infos)
    } else {
        invoke_signed(&ix, &infos, &[seeds])
    }
}
pub fn process(program: &Pubkey, a: &[AccountInfo], data: &[u8]) -> ProgramResult {
    check(crate::DEPLOYMENT_NETWORK == "devnet", Error::InvalidNetwork)?;
    let ix: Instruction =
        borsh::from_slice(data).map_err(|_| ProgramError::InvalidInstructionData)?;
    match ix {
        Instruction::Activate(t) => {
            check(a.len() == 11, Error::InvalidAccount)?;
            let (state, owner, compiler, source, vault, dest, m, tokenp, clock, system, rent) = (
                &a[0], &a[1], &a[2], &a[3], &a[4], &a[5], &a[6], &a[7], &a[8], &a[9], &a[10],
            );
            check(owner.is_signer, Error::Unauthorized)?;
            signer(compiler, &t.compiler)?;
            check(
                [
                    t.policy_id,
                    t.executor,
                    t.compiler,
                    t.recipient,
                    t.service_hash,
                    t.source_hash,
                    t.ir_hash,
                ]
                .iter()
                .all(|x| *x != [0; 32])
                    && t.allocation > 0
                    && t.per_call > 0
                    && t.per_call <= t.allocation
                    && t.expires_at > now(clock)?,
                Error::InvalidTerms,
            )?;
            let (key, bump) = address(program, owner.key, &t.policy_id);
            check(
                *state.key == key
                    && t.recipient != key.to_bytes()
                    && t.recipient != owner.key.to_bytes(),
                Error::InvalidAccount,
            )?;
            token_program(tokenp)?;
            let mint_key = mint(m)?;
            token(source, owner.key, &mint_key, true)?;
            let vault_token = token(vault, state.key, &mint_key, true)?;
            check(
                vault_token.delegate == COption::None
                    && vault_token.close_authority == COption::None,
                Error::InvalidAccount,
            )?;
            token(dest, &Pubkey::new_from_array(t.recipient), &mint_key, false)?;
            create(
                program,
                state,
                owner,
                system,
                rent,
                STATE_BYTES,
                &[b"allowit-vault", owner.key.as_ref(), &t.policy_id, &[bump]],
            )?;
            transfer(source, vault, m, owner, tokenp, t.allocation, &[])?;
            write_state(
                state,
                &State {
                    policy_id: t.policy_id,
                    owner: owner.key.to_bytes(),
                    executor: t.executor,
                    compiler: t.compiler,
                    recipient: t.recipient,
                    service_hash: t.service_hash,
                    source_hash: t.source_hash,
                    ir_hash: t.ir_hash,
                    allocation: t.allocation,
                    per_call: t.per_call,
                    expires_at: t.expires_at,
                    spent: 0,
                    nonce: 0,
                    revoked: false,
                },
            )
        }
        Instruction::Execute {
            amount,
            nonce,
            challenge_hash,
            request_hash,
            expires_at,
        } => {
            check(a.len() == 10, Error::InvalidAccount)?;
            let (state, executor, vault, dest, m, tokenp, clock, charge, system, rent) = (
                &a[0], &a[1], &a[2], &a[3], &a[4], &a[5], &a[6], &a[7], &a[8], &a[9],
            );
            check(state.is_writable, Error::InvalidAccount)?;
            let mut s = read_state(state, program)?;
            signer(executor, &s.executor)?;
            check_payment(&s, amount, nonce, expires_at, now(clock)?)?;
            check(
                challenge_hash != [0; 32] && request_hash != [0; 32],
                Error::InvalidTerms,
            )?;
            token_program(tokenp)?;
            let mint_key = mint(m)?;
            let v = token(vault, state.key, &mint_key, true)?;
            check(
                v.delegate == COption::None && v.close_authority == COption::None,
                Error::InvalidAccount,
            )?;
            token(dest, &Pubkey::new_from_array(s.recipient), &mint_key, true)?;
            let (charge_key, bump) = charge_address(program, state.key, &challenge_hash);
            check(*charge.key == charge_key, Error::InvalidAccount)?;
            check(
                charge.owner == &system_program::ID && charge.data_is_empty(),
                Error::Replay,
            )?;
            create(
                program,
                charge,
                executor,
                system,
                rent,
                CHARGE_BYTES,
                &[
                    b"allowit-charge",
                    state.key.as_ref(),
                    &challenge_hash,
                    &[bump],
                ],
            )?;
            let owner = Pubkey::new_from_array(s.owner);
            let (_, vault_bump) = address(program, &owner, &s.policy_id);
            transfer(
                vault,
                dest,
                m,
                state,
                tokenp,
                amount,
                &[
                    b"allowit-vault",
                    owner.as_ref(),
                    &s.policy_id,
                    &[vault_bump],
                ],
            )?;
            let mut marker = charge.try_borrow_mut_data()?;
            marker[..8].copy_from_slice(b"ALCHG001");
            marker[8..40].copy_from_slice(state.key.as_ref());
            marker[40..72].copy_from_slice(&challenge_hash);
            marker[72..104].copy_from_slice(&request_hash);
            marker[104..112].copy_from_slice(&amount.to_le_bytes());
            marker[112..120].copy_from_slice(&nonce.to_le_bytes());
            s.spent += amount;
            s.nonce = nonce;
            write_state(state, &s)
        }
        Instruction::Revoke => {
            check(a.len() == 2, Error::InvalidAccount)?;
            let mut s = read_state(&a[0], program)?;
            signer(&a[1], &s.owner)?;
            s.revoked = true;
            write_state(&a[0], &s)
        }
        Instruction::WithdrawRemaining => {
            check(a.len() == 6, Error::InvalidAccount)?;
            let (state, owner, vault, dest, m, tokenp) = (&a[0], &a[1], &a[2], &a[3], &a[4], &a[5]);
            let mut s = read_state(state, program)?;
            signer(owner, &s.owner)?;
            token_program(tokenp)?;
            let mint_key = mint(m)?;
            let v = token(vault, state.key, &mint_key, true)?;
            token(dest, owner.key, &mint_key, true)?;
            let (_, bump) = address(program, owner.key, &s.policy_id);
            if v.amount > 0 {
                transfer(
                    vault,
                    dest,
                    m,
                    state,
                    tokenp,
                    v.amount,
                    &[b"allowit-vault", owner.key.as_ref(), &s.policy_id, &[bump]],
                )?;
            }
            s.revoked = true;
            write_state(state, &s)
        }
    }
}
