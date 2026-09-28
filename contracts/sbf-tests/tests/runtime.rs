//! These tests require the actual `cargo build-sbf` ELF, never a native stub.
#[path = "../../test_support.rs"]
mod support;

use allowit_contract_core::{Error, EvidenceAuthority, Request, State, mandate_hash};
use allowit_solana::{Instruction, MAX_SOLANA_ARTIFACT_BYTES, STATE_BYTES};
use mollusk_svm::{
    Mollusk,
    result::{InstructionResult, ProgramResult},
};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction as SvmInstruction};
use solana_program::{program_option::COption, program_pack::Pack, pubkey::Pubkey as LegacyKey};
use solana_program_error::ProgramError;
use solana_pubkey::Pubkey;
use spl_token::state::{Account as TokenAccount, AccountState, Mint};
use std::collections::BTreeMap;

fn modern(key: LegacyKey) -> Pubkey {
    Pubkey::new_from_array(key.to_bytes())
}
fn key(bytes: [u8; 32]) -> Pubkey {
    Pubkey::new_from_array(bytes)
}
fn ro(key: Pubkey, signer: bool) -> AccountMeta {
    AccountMeta::new_readonly(key, signer)
}
fn rw(key: Pubkey, signer: bool) -> AccountMeta {
    AccountMeta::new(key, signer)
}
fn account(owner: Pubkey, data: Vec<u8>) -> Account {
    Account {
        lamports: 10_000_000_000,
        data,
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

struct Fixture {
    svm: Mollusk,
    accounts: BTreeMap<Pubkey, Account>,
    program: Pubkey,
    state_key: Pubkey,
    head: Pubkey,
    source: Pubkey,
    destination: Pubkey,
    delegate: Pubkey,
    state: State,
}

impl Fixture {
    fn new(source_code: &str, evidence: bool) -> Self {
        Self::with_policy(support::fixture(source_code), evidence)
    }
    fn with_policy(mut state: State, evidence: bool) -> Self {
        // Missing ELF is a hard failure: this suite cannot silently become native.
        assert!(
            std::env::var_os("SBF_OUT_DIR").is_some(),
            "Set SBF_OUT_DIR to the compiled SBF artifact directory"
        );
        let program = key([17; 32]);
        let state_key = key([18; 32]);
        let mut svm = Mollusk::new(&program, "allowit_solana");
        svm.compute_budget.compute_unit_limit = u64::from(allowit_solana::REQUIRED_COMPUTE_UNITS);
        svm.compute_budget.heap_size = allowit_solana::REQUIRED_HEAP_BYTES;
        svm.sysvars.clock.unix_timestamp = 1000;
        mollusk_svm_programs_token::token::add_program(&mut svm);
        state.mandate.network = allowit_solana::network_label().into();
        state.mandate.asset = allowit_solana::canonical_usdc().unwrap().to_bytes();
        state.mandate.recipient_address =
            LegacyKey::new_from_array(state.mandate.recipient).to_string();
        if evidence {
            state.mandate.evidence_authority = Some(EvidenceAuthority {
                key: [6; 32],
                key_id: "evidence-v1".into(),
                version: "1".into(),
            });
        }
        let legacy_program = LegacyKey::new_from_array(program.to_bytes());
        let delegate = modern(
            allowit_solana::delegate_address(
                &legacy_program,
                &LegacyKey::new_from_array(state_key.to_bytes()),
            )
            .0,
        );
        let head = modern(
            allowit_solana::head_address(
                &legacy_program,
                &LegacyKey::new_from_array(state.mandate.owner),
                &state.mandate.policy_id,
            )
            .0,
        );
        let source = key([19; 32]);
        let destination = key([20; 32]);
        let mint = LegacyKey::new_from_array(state.mandate.asset);
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
        let token_data = |owner, amount, delegated| {
            let mut data = vec![0; TokenAccount::LEN];
            TokenAccount::pack(
                TokenAccount {
                    mint,
                    owner,
                    amount,
                    delegate: if delegated {
                        COption::Some(LegacyKey::new_from_array(delegate.to_bytes()))
                    } else {
                        COption::None
                    },
                    delegated_amount: if delegated { 100_000_000 } else { 0 },
                    state: AccountState::Initialized,
                    is_native: COption::None,
                    close_authority: COption::None,
                },
                &mut data,
            )
            .unwrap();
            data
        };
        let token = modern(spl_token::ID);
        let mut accounts = BTreeMap::new();
        accounts.insert(state_key, account(program, vec![0; STATE_BYTES]));
        // A prefunded PDA must still be initializable via real System CPI.
        accounts.insert(
            head,
            Account {
                lamports: 1,
                ..Account::default()
            },
        );
        for identity in [
            state.mandate.owner,
            state.mandate.compiler,
            state.mandate.executor,
            [6; 32],
            delegate.to_bytes(),
        ] {
            accounts.insert(key(identity), account(Pubkey::default(), vec![]));
        }
        accounts.insert(key(state.mandate.asset), account(token, mint_data));
        accounts.insert(
            source,
            account(
                token,
                token_data(
                    LegacyKey::new_from_array(state.mandate.owner),
                    200_000_000,
                    true,
                ),
            ),
        );
        accounts.insert(
            destination,
            account(
                token,
                token_data(LegacyKey::new_from_array(state.mandate.recipient), 0, false),
            ),
        );
        for (key, account) in [
            svm.sysvars.keyed_account_for_clock_sysvar(),
            svm.sysvars.keyed_account_for_rent_sysvar(),
            mollusk_svm_programs_token::token::keyed_account(),
            mollusk_svm::program::keyed_account_for_system_program(),
        ] {
            accounts.insert(key, account);
        }
        Self {
            svm,
            accounts,
            program,
            state_key,
            head,
            source,
            destination,
            delegate,
            state,
        }
    }

    fn run(
        &mut self,
        label: &str,
        instruction: Instruction,
        metas: Vec<AccountMeta>,
    ) -> InstructionResult {
        let ix = SvmInstruction {
            program_id: self.program,
            accounts: metas,
            data: borsh::to_vec(&instruction).unwrap(),
        };
        assert!(
            ix.data.len() <= 1024,
            "{label} has {} instruction bytes",
            ix.data.len()
        );
        let before: Vec<_> = self.accounts.iter().map(|(k, a)| (*k, a.clone())).collect();
        let result = self.svm.process_instruction(&ix, &before);
        println!(
            "SBF {label}: artifact={}B instruction={}B CU={} result={:?}",
            self.state.artifact.len(),
            ix.data.len(),
            result.compute_units_consumed,
            result.program_result
        );
        assert!(result.compute_units_consumed <= u64::from(allowit_solana::REQUIRED_COMPUTE_UNITS));
        if result.program_result.is_ok() {
            for (k, a) in &result.resulting_accounts {
                self.accounts.insert(*k, a.clone());
            }
        } else {
            // Check the VM output, not merely our fixture's discarded state.
            for checked in [self.state_key, self.head, self.source, self.destination] {
                assert_eq!(
                    result.get_account(&checked).unwrap(),
                    self.accounts.get(&checked).unwrap(),
                    "{label} changed an account on failure"
                );
            }
        }
        result
    }
    fn ok(&mut self, label: &str, instruction: Instruction, metas: Vec<AccountMeta>) {
        assert_eq!(
            self.run(label, instruction, metas).program_result,
            ProgramResult::Success,
            "{label}"
        );
    }
    fn initialize(&mut self) {
        let m = self.state.mandate.clone();
        self.ok(
            "initialize-head",
            Instruction::InitializeHead {
                policy_id: m.policy_id,
            },
            vec![
                rw(self.head, false),
                rw(key(m.owner), true),
                ro(modern(solana_program::sysvar::rent::ID), false),
                ro(Pubkey::default(), false),
            ],
        );
        self.initialize_revision();
    }
    fn initialize_revision(&mut self) {
        let m = self.state.mandate.clone();
        self.ok(
            "initialize",
            Instruction::Initialize { mandate: m.clone() },
            vec![
                rw(self.state_key, true),
                ro(key(m.owner), true),
                ro(modern(solana_program::sysvar::rent::ID), false),
                ro(self.head, false),
            ],
        );
        let artifact = self.state.artifact.clone();
        for (i, bytes) in artifact.chunks(700).enumerate() {
            self.ok(
                "upload",
                Instruction::Upload {
                    offset: (i * 700) as u32,
                    bytes: bytes.to_vec(),
                },
                vec![rw(self.state_key, false), ro(key(m.owner), true)],
            );
        }
    }
    fn activate(&mut self, compiler_signer: bool) -> InstructionResult {
        let m = &self.state.mandate;
        self.run(
            "activate",
            Instruction::Activate {
                expected_mandate_hash: mandate_hash(m).unwrap(),
            },
            vec![
                rw(self.state_key, false),
                ro(key(m.owner), true),
                ro(key(m.compiler), compiler_signer),
                ro(modern(solana_program::sysvar::clock::ID), false),
                rw(self.head, false),
            ],
        )
    }
    fn ready(source: &str, evidence: bool) -> Self {
        let mut f = Self::new(source, evidence);
        f.initialize();
        assert_eq!(f.activate(true).program_result, ProgramResult::Success);
        f
    }
    fn execute_metas(&self, signed: bool) -> Vec<AccountMeta> {
        let mut metas = vec![
            rw(self.state_key, false),
            ro(key(self.state.mandate.executor), true),
            ro(self.delegate, false),
            rw(self.source, false),
            rw(self.destination, false),
            ro(key(self.state.mandate.asset), false),
            ro(modern(spl_token::ID), false),
            ro(modern(solana_program::sysvar::clock::ID), false),
            ro(self.head, false),
        ];
        if self.state.mandate.evidence_authority.is_some() {
            metas.push(ro(key([6; 32]), signed));
        }
        metas
    }
    fn execute(&mut self, request: Request, signed: bool) -> InstructionResult {
        self.run(
            "execute",
            Instruction::Execute { request },
            self.execute_metas(signed),
        )
    }
    fn read_state(&self) -> State {
        let bytes = &self.accounts[&self.state_key].data;
        let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        borsh::from_slice(&bytes[12..12 + len]).unwrap()
    }
    fn balance(&self) -> u64 {
        TokenAccount::unpack(&self.accounts[&self.destination].data)
            .unwrap()
            .amount
    }
    fn error(result: InstructionResult, error: Error) {
        assert_eq!(
            result.program_result,
            ProgramResult::Failure(ProgramError::Custom(error as u32))
        );
    }
}

#[test]
fn real_sbf_transfer_checks_identity_scope_budget_nonce_and_revocation() {
    let mut f = Fixture::new(support::SIMPLE, false);
    f.initialize();
    Fixture::error(f.activate(false), Error::Unauthorized);
    assert_eq!(f.activate(true).program_result, ProgramResult::Success);
    for field in 0..8 {
        let mut request = support::request(&f.state, 10_000_000);
        match field {
            0 => request.asset[0] ^= 1,
            1 => request.recipient[0] ^= 1,
            2 => request.action = "other".into(),
            3 => request.network = "solana:mainnet".into(),
            4 => request.revision += 1,
            5 => request.source_hash = "0".repeat(64),
            6 => request.ir_hash = "0".repeat(64),
            _ => request.merchant = "other".into(),
        }
        Fixture::error(f.execute(request, false), Error::BindingMismatch);
    }
    let mut metas = f.execute_metas(false);
    metas[1].is_signer = false;
    Fixture::error(
        f.run(
            "unsigned-executor",
            Instruction::Execute {
                request: support::request(&f.state, 1_000_000),
            },
            metas,
        ),
        Error::Unauthorized,
    );
    Fixture::error(
        f.execute(support::request(&f.state, 11_000_000), false),
        Error::PolicyDenied,
    );
    let first = support::request(&f.state, 10_000_000);
    assert_eq!(
        f.execute(first.clone(), false).program_result,
        ProgramResult::Success
    );
    assert_eq!(f.balance(), 10_000_000);
    Fixture::error(f.execute(first, false), Error::Replay);
    for _ in 1..10 {
        assert_eq!(
            f.execute(support::request(&f.read_state(), 10_000_000), false)
                .program_result,
            ProgramResult::Success
        );
    }
    // Renewing token allowance does not renew the policy allocation.
    let mut source = TokenAccount::unpack(&f.accounts[&f.source].data).unwrap();
    source.delegated_amount = 100_000_000;
    source.delegate = COption::Some(LegacyKey::new_from_array(f.delegate.to_bytes()));
    TokenAccount::pack(source, &mut f.accounts.get_mut(&f.source).unwrap().data).unwrap();
    Fixture::error(
        f.execute(support::request(&f.read_state(), 1), false),
        Error::BudgetExceeded,
    );
    f.ok(
        "revoke",
        Instruction::Revoke,
        vec![rw(f.state_key, false), ro(key(f.state.mandate.owner), true)],
    );
    Fixture::error(
        f.execute(support::request(&f.read_state(), 1), false),
        Error::Inactive,
    );
}

#[test]
fn actual_sbf_rejects_user_input_and_unauthorized_or_substituted_evidence() {
    let mut input = Fixture::ready(support::INPUT, false);
    Fixture::error(
        input.execute(support::request(&input.state, 1_000_000), false),
        Error::UserInputRequired,
    );
    assert_eq!(input.balance(), 0);
    let mut f = Fixture::ready(support::CONFIDENCE, true);
    Fixture::error(
        f.execute(support::request(&f.state, 1_000_000), false),
        Error::EvidenceRequired,
    );
    let mut request = support::request(&f.state, 1_000_000);
    support::evidence(&f.state, &mut request, 9500, 9800);
    Fixture::error(f.execute(request.clone(), false), Error::Unauthorized);
    for field in 0..4 {
        let mut bad = request.clone();
        match field {
            0 => bad.amount_units += 1,
            1 => bad.evidence.as_mut().unwrap().expires_at = 999,
            2 => bad.evidence.as_mut().unwrap().intervals[0].lower_bps = 9900,
            _ => bad.evidence.as_mut().unwrap().intervals[0].upper_bps = 10_001,
        }
        Fixture::error(f.execute(bad, true), Error::InvalidEvidence);
    }
    let mut uncertain = request.clone();
    uncertain.evidence.as_mut().unwrap().intervals[0].lower_bps = 8000;
    Fixture::error(f.execute(uncertain, true), Error::UserInputRequired);
    assert_eq!(
        f.execute(request, true).program_result,
        ProgramResult::Success
    );
    assert_eq!(f.balance(), 1_000_000);
}

#[test]
fn semantic_request_covers_runtime_context_and_immutable_intent_in_sbf() {
    let mut f = Fixture::ready(support::SEMANTIC, true);
    let mut request = support::request(&f.state, 1_000_000);
    request.runtime_context = r#"{"risk":2}"#.into();
    Fixture::error(f.execute(request.clone(), true), Error::EvidenceRequired);
    support::semantic_evidence(&f.state, &mut request, 9500);
    let mut altered = request.clone();
    altered.runtime_context = r#"{"risk":3}"#.into();
    Fixture::error(f.execute(altered, true), Error::InvalidEvidence);
    assert_eq!(
        f.execute(request, true).program_result,
        ProgramResult::Success
    );
}

#[test]
fn actual_vm_rejects_wrong_token_program_mint_owners_and_delegate() {
    let mut f = Fixture::ready(support::SIMPLE, false);
    for index in [2, 5, 6] {
        let mut metas = f.execute_metas(false);
        metas[index] = ro(key([6; 32]), false);
        Fixture::error(
            f.run(
                "substituted-account",
                Instruction::Execute {
                    request: support::request(&f.state, 1_000_000),
                },
                metas,
            ),
            Error::InvalidAccount,
        );
    }
    for (which, change_delegate) in [(f.source, false), (f.destination, false), (f.source, true)] {
        let original = f.accounts[&which].clone();
        let mut token = TokenAccount::unpack(&original.data).unwrap();
        if change_delegate {
            token.delegate = COption::Some(LegacyKey::new_from_array([7; 32]));
        } else {
            token.owner = LegacyKey::new_from_array([7; 32]);
        }
        TokenAccount::pack(token, &mut f.accounts.get_mut(&which).unwrap().data).unwrap();
        Fixture::error(
            f.execute(support::request(&f.state, 1_000_000), false),
            Error::BindingMismatch,
        );
        f.accounts.insert(which, original);
    }
    // Real token failure must not consume the policy nonce or budget.
    let mut source = TokenAccount::unpack(&f.accounts[&f.source].data).unwrap();
    source.amount = 0;
    TokenAccount::pack(source, &mut f.accounts.get_mut(&f.source).unwrap().data).unwrap();
    assert!(
        f.execute(support::request(&f.state, 1_000_000), false)
            .program_result
            .is_err()
    );
    assert_eq!(f.read_state().next_nonce, 0);
}

#[test]
fn latest_revision_supersedes_the_old_delegate_on_the_actual_vm() {
    let mut f = Fixture::ready(support::SIMPLE, false);
    let old_key = f.state_key;
    let old_delegate = f.delegate;
    let old_state = f.state.clone();
    f.state_key = key([21; 32]);
    f.state.mandate.revision = 2;
    f.accounts
        .insert(f.state_key, account(f.program, vec![0; STATE_BYTES]));
    f.initialize_revision();
    assert_eq!(f.activate(true).program_result, ProgramResult::Success);
    f.state_key = old_key;
    f.delegate = old_delegate;
    f.state = old_state;
    Fixture::error(
        f.execute(support::request(&f.state, 1_000_000), false),
        Error::Inactive,
    );
    assert_eq!(f.balance(), 0);
}

#[test]
fn maximum_artifact_and_eight_level_semantic_ir_execute_within_sbf_limits() {
    let mut f = Fixture::with_policy(support::maximum_semantic_fixture(), true);
    assert_eq!(f.state.artifact.len(), MAX_SOLANA_ARTIFACT_BYTES);
    f.initialize();
    assert_eq!(f.activate(true).program_result, ProgramResult::Success);
    let mut request = support::request(&f.state, 1_000_000);
    request.runtime_context = r#"{"risk":2}"#.into();
    support::semantic_evidence(&f.state, &mut request, 9500);
    assert_eq!(
        f.execute(request, true).program_result,
        ProgramResult::Success
    );
    assert_eq!(f.balance(), 1_000_000);
}

#[test]
fn full_v0_transaction_with_three_signature_slots_fits_the_packet() {
    use solana_message::{AddressLookupTableAccount, VersionedMessage, v0};
    use solana_transaction::versioned::VersionedTransaction;
    let f = Fixture::new(support::SEMANTIC, true);
    let mut request = support::request(&f.state, 1_000_000);
    let context = serde_json::json!({"note":"", "risk":2});
    let overhead = serde_json::to_string(&context).unwrap().len();
    request.runtime_context = serde_json::to_string(&serde_json::json!({
        "note":"x".repeat(allowit_solana::MAX_RUNTIME_CONTEXT_BYTES - overhead), "risk":2,
    }))
    .unwrap();
    assert_eq!(
        request.runtime_context.len(),
        allowit_solana::MAX_RUNTIME_CONTEXT_BYTES
    );
    support::semantic_evidence(&f.state, &mut request, 9500);
    let mut instructions: Vec<_> = allowit_solana::required_compute_budget_instructions()
        .into_iter()
        .map(|ix| SvmInstruction {
            program_id: modern(ix.program_id),
            accounts: vec![],
            data: ix.data,
        })
        .collect();
    instructions.push(SvmInstruction {
        program_id: f.program,
        accounts: f.execute_metas(true),
        data: borsh::to_vec(&Instruction::Execute { request }).unwrap(),
    });
    let lookup = AddressLookupTableAccount {
        key: key([30; 32]),
        addresses: instructions
            .last()
            .unwrap()
            .accounts
            .iter()
            .filter(|meta| !meta.is_signer)
            .map(|meta| meta.pubkey)
            .collect(),
    };
    let message = v0::Message::try_compile(
        &key(f.state.mandate.owner),
        &instructions,
        &[lookup],
        Default::default(),
    )
    .unwrap();
    assert_eq!(message.header.num_required_signatures, 3);
    let transaction = VersionedTransaction {
        // Wire-size evidence uses full 64-byte signature slots; it does not
        // claim a real wallet signature or an RPC submission.
        signatures: vec![Default::default(); 3],
        message: VersionedMessage::V0(message),
    };
    let bytes = bincode::serialize(&transaction).unwrap();
    println!(
        "Solana complete v0 packet: {} bytes, runtime context: {} bytes, signatures: 3",
        bytes.len(),
        allowit_solana::MAX_RUNTIME_CONTEXT_BYTES
    );
    assert!(
        bytes.len() <= 1232,
        "Packet exceeds Solana's 1232-byte transaction maximum"
    );
}

#[test]
fn logical_head_cannot_be_substituted_reinitialized_or_used_to_skip_revisions() {
    let mut f = Fixture::new(support::SIMPLE, false);
    let owner = key(f.state.mandate.owner);
    let head_metas = |actor, signer| {
        vec![
            rw(f.head, false),
            rw(actor, signer),
            ro(modern(solana_program::sysvar::rent::ID), false),
            ro(Pubkey::default(), false),
        ]
    };
    let unsigned = head_metas(owner, false);
    let other = head_metas(key(f.state.mandate.compiler), true);
    Fixture::error(
        f.run(
            "unsigned-head-owner",
            Instruction::InitializeHead {
                policy_id: f.state.mandate.policy_id,
            },
            unsigned,
        ),
        Error::Unauthorized,
    );
    Fixture::error(
        f.run(
            "other-head-owner",
            Instruction::InitializeHead {
                policy_id: f.state.mandate.policy_id,
            },
            other,
        ),
        Error::InvalidAccount,
    );
    f.initialize();
    Fixture::error(
        f.run(
            "reinitialize-head",
            Instruction::InitializeHead {
                policy_id: f.state.mandate.policy_id,
            },
            vec![
                rw(f.head, false),
                rw(owner, true),
                ro(modern(solana_program::sysvar::rent::ID), false),
                ro(Pubkey::default(), false),
            ],
        ),
        Error::InvalidAccount,
    );
    let m = f.state.mandate.clone();
    Fixture::error(
        f.run(
            "wrong-head",
            Instruction::Activate {
                expected_mandate_hash: mandate_hash(&m).unwrap(),
            },
            vec![
                rw(f.state_key, false),
                ro(owner, true),
                ro(key(m.compiler), true),
                ro(modern(solana_program::sysvar::clock::ID), false),
                rw(key([6; 32]), false),
            ],
        ),
        Error::InvalidAccount,
    );
    assert_eq!(f.activate(true).program_result, ProgramResult::Success);
    Fixture::error(f.activate(true), Error::BindingMismatch);
    f.state_key = key([22; 32]);
    f.state.mandate.revision = 3;
    f.accounts
        .insert(f.state_key, account(f.program, vec![0; STATE_BYTES]));
    f.initialize_revision();
    Fixture::error(f.activate(true), Error::BindingMismatch);
}
