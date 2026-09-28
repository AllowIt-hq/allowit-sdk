#[path = "../../test_support.rs"]
mod support;

#[test]
fn unavailable_canonical_usdc_cannot_activate_on_solana_testnet() {
    if allowit_solana::DEPLOYMENT_NETWORK != "testnet" {
        return;
    }
    use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};
    let program = Pubkey::new_from_array([17; 32]);
    let state_key = Pubkey::new_from_array([18; 32]);
    let owner_key = Pubkey::new_from_array([1; 32]);
    let system = Pubkey::default();
    let mut state_lamports = 1_000_000_000;
    let mut owner_lamports = 1_000_000_000;
    let mut state_data = vec![0; allowit_solana::STATE_BYTES];
    let mut owner_data = vec![];
    let state = AccountInfo::new(
        &state_key,
        true,
        true,
        &mut state_lamports,
        &mut state_data,
        &program,
        false,
        0,
    );
    let owner = AccountInfo::new(
        &owner_key,
        true,
        false,
        &mut owner_lamports,
        &mut owner_data,
        &system,
        false,
        0,
    );
    let mut mandate = support::fixture(support::SIMPLE).mandate;
    mandate.network = "solana:testnet".into();
    mandate.recipient_address = Pubkey::new_from_array(mandate.recipient).to_string();
    assert!(allowit_solana::canonical_usdc().is_none());
    let bytes = borsh::to_vec(&allowit_solana::Instruction::Initialize { mandate }).unwrap();
    assert_eq!(
        allowit_solana::process_instruction(&program, &[state, owner], &bytes),
        Err(ProgramError::Custom(
            allowit_contract_core::Error::BindingMismatch as u32
        ))
    );
    println!("Solana Testnet rejects mandate initialization without canonical USDC");
}
