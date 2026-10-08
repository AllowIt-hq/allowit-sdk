//! Real SBF lifecycle and adversarial payment tests (no native CPI success stubs).
use allowit_solana::vault::{self, Error, Instruction, State, Terms};
use mollusk_svm::{Mollusk, result::ProgramResult};
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction as Ix};
use solana_program::{program_option::COption, program_pack::Pack, pubkey::Pubkey as OldKey};
use solana_pubkey::Pubkey;
use spl_token::state::{Account as Token, AccountState, Mint};
use std::collections::BTreeMap;
fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}
fn old(k: Pubkey) -> OldKey {
    OldKey::new_from_array(k.to_bytes())
}
fn modern(k: OldKey) -> Pubkey {
    Pubkey::new_from_array(k.to_bytes())
}
fn rw(k: Pubkey, s: bool) -> AccountMeta {
    AccountMeta::new(k, s)
}
fn ro(k: Pubkey, s: bool) -> AccountMeta {
    AccountMeta::new_readonly(k, s)
}
fn account(owner: Pubkey, data: Vec<u8>) -> Account {
    Account {
        lamports: 10_000_000_000,
        owner,
        data,
        ..Account::default()
    }
}
struct F {
    vm: Mollusk,
    accounts: BTreeMap<Pubkey, Account>,
    program: Pubkey,
    state: Pubkey,
    owner: Pubkey,
    executor: Pubkey,
    compiler: Pubkey,
    mint: Pubkey,
    source: Pubkey,
    vault: Pubkey,
    dest: Pubkey,
    terms: Terms,
}
impl F {
    fn new() -> Self {
        assert!(
            std::env::var_os("SBF_OUT_DIR").is_some(),
            "compiled ELF required"
        );
        let program = key(71);
        let owner = key(72);
        let executor = key(73);
        let compiler = key(74);
        let recipient = key(75);
        let terms = Terms {
            policy_id: [76; 32],
            executor: executor.to_bytes(),
            compiler: compiler.to_bytes(),
            recipient: recipient.to_bytes(),
            service_hash: [77; 32],
            source_hash: [78; 32],
            ir_hash: [79; 32],
            allocation: 100_000,
            per_call: 60_000,
            expires_at: 2000,
        };
        let state = modern(vault::address(&old(program), &old(owner), &terms.policy_id).0);
        let mint = modern(allowit_solana::canonical_usdc().unwrap());
        let source = modern(vault::ata(&old(owner), &old(mint)));
        let vault = modern(vault::ata(&old(state), &old(mint)));
        let dest = modern(vault::ata(&old(recipient), &old(mint)));
        let mut vm = Mollusk::new(&program, "allowit_solana");
        vm.compute_budget.compute_unit_limit = 1_400_000;
        vm.compute_budget.heap_size = 256 * 1024;
        vm.sysvars.clock.unix_timestamp = 1000;
        mollusk_svm_programs_token::token::add_program(&mut vm);
        let mut accounts = BTreeMap::new();
        for k in [owner, executor, compiler] {
            accounts.insert(k, account(Pubkey::default(), vec![]));
        }
        accounts.insert(
            state,
            Account {
                lamports: 1,
                ..Account::default()
            },
        );
        let mut md = vec![0; Mint::LEN];
        Mint::pack(
            Mint {
                mint_authority: COption::None,
                supply: 1_000_000,
                decimals: 6,
                is_initialized: true,
                freeze_authority: COption::None,
            },
            &mut md,
        )
        .unwrap();
        accounts.insert(mint, account(modern(spl_token::ID), md));
        for (k, o, amount) in [
            (source, owner, 1_000_000),
            (vault, state, 0),
            (dest, recipient, 0),
        ] {
            let mut d = vec![0; Token::LEN];
            Token::pack(
                Token {
                    mint: old(mint),
                    owner: old(o),
                    amount,
                    delegate: COption::None,
                    state: AccountState::Initialized,
                    is_native: COption::None,
                    delegated_amount: 0,
                    close_authority: COption::None,
                },
                &mut d,
            )
            .unwrap();
            accounts.insert(k, account(modern(spl_token::ID), d));
        }
        for (k, a) in [
            vm.sysvars.keyed_account_for_clock_sysvar(),
            vm.sysvars.keyed_account_for_rent_sysvar(),
            mollusk_svm_programs_token::token::keyed_account(),
            mollusk_svm::program::keyed_account_for_system_program(),
        ] {
            accounts.insert(k, a);
        }
        Self {
            vm,
            accounts,
            program,
            state,
            owner,
            executor,
            compiler,
            mint,
            source,
            vault,
            dest,
            terms,
        }
    }
    fn run(&mut self, i: Instruction, metas: Vec<AccountMeta>) -> ProgramResult {
        let ix = Ix {
            program_id: self.program,
            accounts: metas,
            data: vault::encode(&i),
        };
        let before: Vec<_> = self.accounts.iter().map(|(k, a)| (*k, a.clone())).collect();
        let r = self.vm.process_instruction(&ix, &before);
        println!(
            "vault {:?}: {} CU {:?}",
            i, r.compute_units_consumed, r.program_result
        );
        if r.program_result.is_ok() {
            for (k, a) in &r.resulting_accounts {
                self.accounts.insert(*k, a.clone());
            }
        } else {
            for (k, a) in &r.resulting_accounts {
                assert_eq!(
                    a, &self.accounts[k],
                    "failed instruction must roll back {k}"
                );
            }
        }
        r.program_result
    }
    fn activate_metas(&self) -> Vec<AccountMeta> {
        vec![
            rw(self.state, false),
            rw(self.owner, true),
            ro(self.compiler, true),
            rw(self.source, false),
            rw(self.vault, false),
            ro(self.dest, false),
            ro(self.mint, false),
            ro(modern(spl_token::ID), false),
            ro(modern(solana_program::sysvar::clock::ID), false),
            ro(Pubkey::default(), false),
            ro(modern(solana_program::sysvar::rent::ID), false),
        ]
    }
    fn activate(&mut self) {
        assert_eq!(
            self.run(
                Instruction::Activate(self.terms.clone()),
                self.activate_metas()
            ),
            ProgramResult::Success
        );
    }
    fn execute_metas(&mut self, id: u8) -> Vec<AccountMeta> {
        let charge =
            modern(vault::charge_address(&old(self.program), &old(self.state), &[id; 32]).0);
        self.accounts.entry(charge).or_insert(Account {
            lamports: 1,
            ..Account::default()
        });
        vec![
            rw(self.state, false),
            rw(self.executor, true),
            rw(self.vault, false),
            rw(self.dest, false),
            ro(self.mint, false),
            ro(modern(spl_token::ID), false),
            ro(modern(solana_program::sysvar::clock::ID), false),
            rw(charge, false),
            ro(Pubkey::default(), false),
            ro(modern(solana_program::sysvar::rent::ID), false),
        ]
    }
    fn pay(&mut self, amount: u64, nonce: u64, id: u8) -> ProgramResult {
        let metas = self.execute_metas(id);
        self.run(Self::payment(amount, nonce, id), metas)
    }
    fn payment(amount: u64, nonce: u64, id: u8) -> Instruction {
        Instruction::Execute {
            amount,
            nonce,
            challenge_hash: [id; 32],
            request_hash: [80; 32],
            expires_at: 1500,
        }
    }
    fn state(&self) -> State {
        borsh::from_slice(&self.accounts[&self.state].data[8..]).unwrap()
    }
    fn balance(&self, k: Pubkey) -> u64 {
        Token::unpack(&self.accounts[&k].data).unwrap().amount
    }
    fn expect_error(r: ProgramResult, e: Error) {
        assert_eq!(
            r,
            ProgramResult::Failure(solana_program_error::ProgramError::Custom(e as u32))
        );
    }
    fn withdraw_metas(&self) -> Vec<AccountMeta> {
        vec![
            rw(self.state, false),
            ro(self.owner, true),
            rw(self.vault, false),
            rw(self.source, false),
            ro(self.mint, false),
            ro(modern(spl_token::ID), false),
        ]
    }
}
#[test]
fn vault_lifecycle_real_sbf() {
    let mut f = F::new();
    f.activate();
    assert_eq!(f.balance(f.vault), 100_000);
    assert_eq!(f.balance(f.source), 900_000);
    assert_eq!(f.pay(60_000, 1, 81), ProgramResult::Success);
    assert_eq!(f.balance(f.dest), 60_000);
    F::expect_error(f.pay(40_001, 2, 82), Error::Limit);
    // Duplicate challenge is rejected even with a fresh valid nonce.
    F::expect_error(f.pay(1, 2, 81), Error::Replay);
    assert_eq!(f.pay(40_000, 2, 82), ProgramResult::Success);
    F::expect_error(f.pay(1, 3, 83), Error::Limit);
    assert_eq!(
        f.run(Instruction::WithdrawRemaining, f.withdraw_metas()),
        ProgramResult::Success
    );
    assert!(f.state().revoked);
    F::expect_error(f.pay(1, 3, 84), Error::Inactive);
    assert!(
        f.run(Instruction::Activate(f.terms.clone()), f.activate_metas())
            .is_err()
    );
}
#[test]
fn vault_authority_accounts_and_failed_cpi_rollback() {
    let mut f = F::new();
    let mut m = f.activate_metas();
    m[2].is_signer = false;
    F::expect_error(
        f.run(Instruction::Activate(f.terms.clone()), m),
        Error::Unauthorized,
    );
    f.activate();
    let mut m = f.execute_metas(81);
    m[1].is_signer = false;
    F::expect_error(f.run(F::payment(1, 1, 81), m), Error::Unauthorized);
    let mut m = f.execute_metas(81);
    m[3] = rw(f.source, false);
    F::expect_error(f.run(F::payment(1, 1, 81), m), Error::InvalidAccount);
    let mut m = f.execute_metas(81);
    m[2] = rw(f.source, false);
    F::expect_error(f.run(F::payment(1, 1, 81), m), Error::InvalidAccount);
    F::expect_error(f.pay(1, 2, 81), Error::Replay);
    F::expect_error(f.pay(60_001, 1, 81), Error::Limit);
    F::expect_error(f.pay(0, 1, 81), Error::Limit);
    // Drain the test fixture balance externally to force the actual Token program
    // to fail after marker creation. All rent/marker/state changes must roll back.
    let mut t = Token::unpack(&f.accounts[&f.vault].data).unwrap();
    t.amount = 0;
    Token::pack(t, &mut f.accounts.get_mut(&f.vault).unwrap().data).unwrap();
    assert!(f.pay(1, 1, 81).is_err());
    assert_eq!(f.state().spent, 0);
    assert_eq!(f.state().nonce, 0);
    let charge = modern(vault::charge_address(&old(f.program), &old(f.state), &[81; 32]).0);
    assert!(f.accounts[&charge].data.is_empty());
}
#[test]
fn vault_expiry_revocation_withdrawal_and_no_replay_reset() {
    let mut f = F::new();
    f.activate();
    assert_eq!(f.pay(10_000, 1, 81), ProgramResult::Success);
    let mut m = f.withdraw_metas();
    m[1] = ro(f.executor, true);
    F::expect_error(
        f.run(Instruction::WithdrawRemaining, m),
        Error::Unauthorized,
    );
    let mut m = vec![rw(f.state, false), ro(f.executor, true)];
    F::expect_error(f.run(Instruction::Revoke, m.clone()), Error::Unauthorized);
    m[1] = ro(f.owner, true);
    assert_eq!(f.run(Instruction::Revoke, m), ProgramResult::Success);
    F::expect_error(f.pay(1, 2, 82), Error::Inactive);
    f.vm.sysvars.clock.unix_timestamp = 3000;
    let (k, a) = f.vm.sysvars.keyed_account_for_clock_sysvar();
    f.accounts.insert(k, a);
    assert_eq!(
        f.run(Instruction::WithdrawRemaining, f.withdraw_metas()),
        ProgramResult::Success
    );
    assert_eq!(f.balance(f.source), 990_000);
    assert_eq!(f.balance(f.vault), 0);
    assert_eq!(f.state().nonce, 1);
    assert_eq!(
        f.run(Instruction::WithdrawRemaining, f.withdraw_metas()),
        ProgramResult::Success
    );
    let mut g = F::new();
    g.activate();
    g.vm.sysvars.clock.unix_timestamp = 1500;
    let (k, a) = g.vm.sysvars.keyed_account_for_clock_sysvar();
    g.accounts.insert(k, a);
    F::expect_error(g.pay(1, 1, 81), Error::Expired);
}
