use allowit_solana::vault::{self, Error, State};
use solana_program::program_error::ProgramError;
fn state() -> State {
    State {
        policy_id: [1; 32],
        owner: [2; 32],
        executor: [3; 32],
        compiler: [4; 32],
        recipient: [5; 32],
        service_hash: [6; 32],
        source_hash: [7; 32],
        ir_hash: [8; 32],
        allocation: 100,
        per_call: 60,
        expires_at: 2000,
        spent: 40,
        nonce: 9,
        revoked: false,
    }
}
#[test]
fn boundaries_overflow_and_terminal_revocation() {
    let mut s = state();
    assert!(vault::check_payment(&s, 60, 10, 1500, 1000).is_ok());
    for (amount, nonce, expiry, now, error) in [
        (61, 10, 1500, 1000, Error::Limit),
        (0, 10, 1500, 1000, Error::Limit),
        (1, 9, 1500, 1000, Error::Replay),
        (1, 11, 1500, 1000, Error::Replay),
        (1, 10, 1500, 1500, Error::Expired),
        (1, 10, 3000, 2000, Error::Expired),
    ] {
        assert_eq!(
            vault::check_payment(&s, amount, nonce, expiry, now),
            Err(ProgramError::Custom(error as u32))
        );
    }
    s.spent = u64::MAX;
    s.allocation = u64::MAX;
    assert_eq!(
        vault::check_payment(&s, 1, 10, 1500, 1000),
        Err(ProgramError::Custom(Error::Limit as u32))
    );
    s = state();
    s.nonce = u64::MAX;
    assert_eq!(
        vault::check_payment(&s, 1, 0, 1500, 1000),
        Err(ProgramError::Custom(Error::Replay as u32))
    );
    s = state();
    s.revoked = true;
    assert_eq!(
        vault::check_payment(&s, 1, 10, 1500, 1000),
        Err(ProgramError::Custom(Error::Inactive as u32))
    );
}
#[test]
fn fixed_wire_layout_and_namespace_remain_distinct() {
    let s = state();
    let bytes = borsh::to_vec(&s).unwrap();
    assert_eq!(bytes.len() + 8, vault::STATE_BYTES);
    assert_eq!(&bytes[..32], &s.policy_id);
    assert_eq!(&bytes[256..264], &100u64.to_le_bytes());
    let e = vault::Instruction::Execute {
        amount: 1,
        nonce: 2,
        challenge_hash: [3; 32],
        request_hash: [4; 32],
        expires_at: 5,
    };
    let wire = vault::encode(&e);
    assert_eq!(wire.len(), 90);
    assert_eq!(&wire[..2], &[0xa1, 1]);
    assert_eq!(&wire[2..10], &1u64.to_le_bytes());
    assert!(borsh::from_slice::<allowit_solana::Instruction>(&wire).is_err());
    assert!(borsh::from_slice::<vault::Instruction>(&[2, 0]).is_err());
}
#[test]
fn non_devnet_vault_wire_is_disabled() {
    if allowit_solana::DEPLOYMENT_NETWORK != "devnet" {
        assert_eq!(
            allowit_solana::process_instruction(
                &solana_program::pubkey::Pubkey::new_unique(),
                &[],
                &vault::encode(&vault::Instruction::Revoke)
            ),
            Err(ProgramError::Custom(Error::InvalidNetwork as u32))
        );
    }
}
