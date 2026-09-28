#[path = "../../test_support.rs"]
mod support;

use allowit_contract_core::{Error, Request, State, mandate_hash};
use allowit_solana::{
    HEAD_BYTES, Instruction, STATE_BYTES, canonical_usdc, delegate_address, head_address,
    process_instruction, read_state,
};
use solana_program::{
    account_info::AccountInfo,
    clock::Clock,
    instruction::Instruction as SolanaInstruction,
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    program_stubs::{SyscallStubs, set_syscall_stubs},
    pubkey::Pubkey,
    rent::Rent,
    sysvar,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

fn account(
    key: Pubkey,
    owner: Pubkey,
    data: Vec<u8>,
    signer: bool,
    writable: bool,
    executable: bool,
) -> AccountInfo<'static> {
    AccountInfo::new(
        Box::leak(Box::new(key)),
        signer,
        writable,
        Box::leak(Box::new(Rent::default().minimum_balance(STATE_BYTES))),
        Box::leak(data.into_boxed_slice()),
        Box::leak(Box::new(owner)),
        executable,
        0,
    )
}

struct TokenCpi {
    program: Pubkey,
}
impl SyscallStubs for TokenCpi {
    fn sol_invoke_signed(
        &self,
        instruction: &SolanaInstruction,
        infos: &[AccountInfo],
        seeds: &[&[&[u8]]],
    ) -> Result<(), ProgramError> {
        assert_eq!(instruction.program_id, spl_token::ID);
        let derived = Pubkey::create_program_address(seeds[0], &self.program).unwrap();
        let mut signed = infos.to_vec();
        for info in &mut signed {
            if *info.key == derived {
                info.is_signer = true;
            }
        }
        // Runs the real classic SPL Token processor in memory, not a success stub.
        spl_token::processor::Processor::process(&spl_token::ID, &signed, &instruction.data)
    }
}

struct Fixture {
    program: Pubkey,
    state: State,
    account: AccountInfo<'static>,
    head: AccountInfo<'static>,
    owner: AccountInfo<'static>,
    compiler: AccountInfo<'static>,
    executor: AccountInfo<'static>,
    attester: AccountInfo<'static>,
    rent: AccountInfo<'static>,
    clock: AccountInfo<'static>,
    delegate: AccountInfo<'static>,
    source: AccountInfo<'static>,
    destination: AccountInfo<'static>,
    mint: AccountInfo<'static>,
    token: AccountInfo<'static>,
}

impl Fixture {
    fn new(source_code: &str) -> Self {
        let program = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut state = support::fixture(source_code);
        state.mandate.network = allowit_solana::network_label().into();
        state.mandate.asset = canonical_usdc().unwrap().to_bytes();
        state.mandate.recipient_address =
            Pubkey::new_from_array(state.mandate.recipient).to_string();
        let (delegate, _) = delegate_address(&program, &state_key);
        let mut head_data = vec![0; HEAD_BYTES];
        head_data[..8].copy_from_slice(b"ALTHD001");
        let head_key = head_address(
            &program,
            &Pubkey::new_from_array(state.mandate.owner),
            &state.mandate.policy_id,
        )
        .0;
        let owner = Pubkey::new_from_array(state.mandate.owner);
        let recipient = Pubkey::new_from_array(state.mandate.recipient);
        let mint = Pubkey::new_from_array(state.mandate.asset);
        let mut mint_data = vec![0; Mint::LEN];
        Mint::pack(
            Mint {
                mint_authority: COption::None,
                supply: 200_000_000,
                decimals: 6,
                is_initialized: true,
                freeze_authority: COption::None,
            },
            &mut mint_data,
        )
        .unwrap();
        let token_account = |owner: Pubkey, amount: u64, delegated: bool| {
            let mut data = vec![0; TokenAccount::LEN];
            TokenAccount::pack(
                TokenAccount {
                    mint,
                    owner,
                    amount,
                    delegate: if delegated {
                        COption::Some(delegate)
                    } else {
                        COption::None
                    },
                    state: AccountState::Initialized,
                    is_native: COption::None,
                    delegated_amount: if delegated { 100_000_000 } else { 0 },
                    close_authority: COption::None,
                },
                &mut data,
            )
            .unwrap();
            account(
                Pubkey::new_unique(),
                spl_token::ID,
                data,
                false,
                true,
                false,
            )
        };
        Self {
            program,
            account: account(state_key, program, vec![0; STATE_BYTES], true, true, false),
            head: account(head_key, program, head_data, false, true, false),
            owner: account(owner, Pubkey::default(), vec![], true, false, false),
            compiler: account(
                Pubkey::new_from_array(state.mandate.compiler),
                Pubkey::default(),
                vec![],
                true,
                false,
                false,
            ),
            executor: account(
                Pubkey::new_from_array(state.mandate.executor),
                Pubkey::default(),
                vec![],
                true,
                false,
                false,
            ),
            attester: account(
                Pubkey::new_from_array([6; 32]),
                Pubkey::default(),
                vec![],
                true,
                false,
                false,
            ),
            rent: account(
                sysvar::rent::ID,
                sysvar::ID,
                bincode::serialize(&Rent::default()).unwrap(),
                false,
                false,
                false,
            ),
            clock: account(
                sysvar::clock::ID,
                sysvar::ID,
                bincode::serialize(&Clock {
                    unix_timestamp: 1000,
                    ..Clock::default()
                })
                .unwrap(),
                false,
                false,
                false,
            ),
            delegate: account(delegate, Pubkey::default(), vec![], false, false, false),
            source: token_account(owner, 200_000_000, true),
            destination: token_account(recipient, 0, false),
            mint: account(mint, spl_token::ID, mint_data, false, false, false),
            token: account(spl_token::ID, Pubkey::default(), vec![], false, false, true),
            state,
        }
    }
    fn initialize(&self) {
        self.run(
            &[
                self.account.clone(),
                self.owner.clone(),
                self.rent.clone(),
                self.head.clone(),
            ],
            Instruction::Initialize {
                mandate: self.state.mandate.clone(),
            },
        )
        .unwrap();
        let mut offset = 0;
        for chunk in self.state.artifact.chunks(700) {
            self.run(
                &[self.account.clone(), self.owner.clone()],
                Instruction::Upload {
                    offset,
                    bytes: chunk.to_vec(),
                },
            )
            .unwrap();
            offset += chunk.len() as u32;
        }
    }
    fn activate(&self) -> Result<(), ProgramError> {
        self.run(
            &[
                self.account.clone(),
                self.owner.clone(),
                self.compiler.clone(),
                self.clock.clone(),
                self.head.clone(),
            ],
            Instruction::Activate {
                expected_mandate_hash: mandate_hash(&self.state.mandate).unwrap(),
            },
        )
    }
    fn run(&self, accounts: &[AccountInfo], instruction: Instruction) -> Result<(), ProgramError> {
        process_instruction(
            &self.program,
            accounts,
            &borsh::to_vec(&instruction).unwrap(),
        )
    }
    fn execute(&self, request: Request) -> Result<(), ProgramError> {
        let mut accounts = vec![
            self.account.clone(),
            self.executor.clone(),
            self.delegate.clone(),
            self.source.clone(),
            self.destination.clone(),
            self.mint.clone(),
            self.token.clone(),
            self.clock.clone(),
            self.head.clone(),
        ];
        if request.evidence.is_some() {
            accounts.push(self.attester.clone());
        }
        self.run(&accounts, Instruction::Execute { request })
    }
    fn balance(&self) -> u64 {
        TokenAccount::unpack(&self.destination.try_borrow_data().unwrap())
            .unwrap()
            .amount
    }
}

#[test]
fn actual_spl_transfer_requires_attestation_signers_binding_budget_and_nonce() {
    let _guard = LOCK.lock().unwrap();
    let mut f = Fixture::new(support::SIMPLE);
    let old_stub = set_syscall_stubs(Box::new(TokenCpi { program: f.program }));
    f.initialize();
    f.compiler.is_signer = false;
    assert_eq!(
        f.activate(),
        Err(ProgramError::Custom(Error::Unauthorized as u32))
    );
    f.compiler.is_signer = true;
    f.activate().unwrap();
    let req = support::request(&f.state, 10_000_000);
    f.executor.is_signer = false;
    assert_eq!(
        f.execute(req.clone()),
        Err(ProgramError::Custom(Error::Unauthorized as u32))
    );
    f.executor.is_signer = true;
    for mutation in 0..5 {
        let mut bad = req.clone();
        match mutation {
            0 => bad.asset[0] ^= 1,
            1 => bad.recipient[0] ^= 1,
            2 => bad.network = "mainnet".into(),
            3 => bad.action = "other".into(),
            _ => bad.revision += 1,
        }
        assert_eq!(
            f.execute(bad),
            Err(ProgramError::Custom(Error::BindingMismatch as u32))
        );
        assert_eq!(f.balance(), 0);
    }
    f.execute(req.clone()).unwrap();
    assert_eq!(f.balance(), 10_000_000);
    assert_eq!(
        read_state(&f.account, &f.program).unwrap().spent_units,
        10_000_000
    );
    assert_eq!(
        f.execute(req),
        Err(ProgramError::Custom(Error::Replay as u32))
    );
    let updated = read_state(&f.account, &f.program).unwrap();
    assert_eq!(
        f.execute(support::request(&updated, 10_000_001)),
        Err(ProgramError::Custom(Error::PolicyDenied as u32))
    );
    f.run(&[f.account.clone(), f.owner.clone()], Instruction::Revoke)
        .unwrap();
    assert_eq!(
        f.execute(support::request(&updated, 1)),
        Err(ProgramError::Custom(Error::Inactive as u32))
    );
    assert_eq!(f.balance(), 10_000_000);
    set_syscall_stubs(old_stub);
}

#[test]
fn reached_user_input_fails_before_transfer_or_state_change() {
    let _guard = LOCK.lock().unwrap();
    let f = Fixture::new(support::INPUT);
    f.initialize();
    f.activate().unwrap();
    let before = f.account.try_borrow_data().unwrap().to_vec();
    assert_eq!(
        f.execute(support::request(&f.state, 1_000_000)),
        Err(ProgramError::Custom(Error::UserInputRequired as u32))
    );
    assert_eq!(f.account.try_borrow_data().unwrap().to_vec(), before);
    assert_eq!(f.balance(), 0);
}

#[test]
fn confidence_requires_bound_signer_and_fresh_request_and_never_skips_input() {
    let _guard = LOCK.lock().unwrap();
    let mut f = Fixture::new(support::CONFIDENCE);
    f.state.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
        key: f.attester.key.to_bytes(),
        key_id: "merchant-oracle-v1".into(),
        version: "1".into(),
    });
    f.initialize();
    f.activate().unwrap();
    let mut req = support::request(&f.state, 1_000_000);
    assert_eq!(
        f.execute(req.clone()),
        Err(ProgramError::Custom(Error::EvidenceRequired as u32))
    );
    support::evidence(&f.state, &mut req, 9200, 9800);
    f.attester.is_signer = false;
    assert_eq!(
        f.execute(req.clone()),
        Err(ProgramError::Custom(Error::Unauthorized as u32))
    );
    f.attester.is_signer = true;
    let mut wrong = req.clone();
    wrong.amount_units += 1;
    assert_eq!(
        f.execute(wrong),
        Err(ProgramError::Custom(Error::InvalidEvidence as u32))
    );
    let mut expired = req.clone();
    expired.evidence.as_mut().unwrap().expires_at = 999;
    assert_eq!(
        f.execute(expired),
        Err(ProgramError::Custom(Error::InvalidEvidence as u32))
    );
    let mut ambiguous = req.clone();
    support::evidence(&f.state, &mut ambiguous, 8000, 9500);
    assert_eq!(
        f.execute(ambiguous),
        Err(ProgramError::Custom(Error::UserInputRequired as u32))
    );
    assert_eq!(f.balance(), 0);
    let old = set_syscall_stubs(Box::new(TokenCpi { program: f.program }));
    f.execute(req).unwrap();
    assert_eq!(f.balance(), 1_000_000);
    set_syscall_stubs(old);
}

#[test]
fn semantic_snapshot_and_runtime_facts_are_attested_on_exact_transfer() {
    let _guard = LOCK.lock().unwrap();
    let mut f = Fixture::new(support::SEMANTIC);
    f.state.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
        key: f.attester.key.to_bytes(),
        key_id: "semantics-v1".into(),
        version: "1".into(),
    });
    f.initialize();
    f.activate().unwrap();
    let mut req = support::request(&f.state, 1_000_000);
    req.runtime_context = "{\"risk\":3}".into();
    support::semantic_evidence(&f.state, &mut req, 9200);
    let mut altered = req.clone();
    altered.runtime_context = "{\"risk\":0}".into();
    assert_eq!(
        f.execute(altered),
        Err(ProgramError::Custom(Error::InvalidEvidence as u32))
    );
    let old = set_syscall_stubs(Box::new(TokenCpi { program: f.program }));
    f.execute(req).unwrap();
    assert_eq!(f.balance(), 1_000_000);
    set_syscall_stubs(old);
}

#[test]
fn newer_revision_invalidates_old_allowance_even_on_another_source_account() {
    let _guard = LOCK.lock().unwrap();
    let old = Fixture::new(support::SIMPLE);
    old.initialize();
    old.activate().unwrap();
    let mut next = Fixture::new(support::SIMPLE);
    next.program = old.program;
    next.account = account(
        Pubkey::new_unique(),
        old.program,
        vec![0; STATE_BYTES],
        true,
        true,
        false,
    );
    next.head = old.head.clone();
    next.state.mandate.revision = 2;
    next.initialize();
    next.activate().unwrap();
    assert_eq!(
        old.execute(support::request(&old.state, 1_000_000)),
        Err(ProgramError::Custom(Error::Inactive as u32))
    );
    assert_eq!(old.balance(), 0);
}

#[test]
fn an_arbitrary_six_decimal_mint_cannot_be_called_usdc() {
    let _guard = LOCK.lock().unwrap();
    let mut f = Fixture::new(support::SIMPLE);
    f.state.mandate.asset = Pubkey::new_unique().to_bytes();
    assert_eq!(
        f.run(
            &[
                f.account.clone(),
                f.owner.clone(),
                f.rent.clone(),
                f.head.clone()
            ],
            Instruction::Initialize {
                mandate: f.state.mandate.clone()
            }
        ),
        Err(ProgramError::Custom(Error::BindingMismatch as u32))
    );
}
