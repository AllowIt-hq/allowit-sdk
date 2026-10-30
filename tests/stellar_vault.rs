use allowit_sdk::{stellar_vault::*, typed_workflow::*};
fn wallet() -> AccountId {
    AccountId {
        chain: Chain::Stellar,
        kind: AccountKind::Wallet,
        address: [1; 32],
    }
}
fn vault() -> AccountId {
    AccountId {
        chain: Chain::Stellar,
        kind: AccountKind::Contract,
        address: [2; 32],
    }
}
#[test]
fn deposit_and_redeem_keep_independent_asset_and_share_units_and_owner_custody() {
    let mut d = VaultDeposit {
        vault: vault(),
        assets: 1_000_000,
        receiver: wallet(),
        from: wallet(),
        operator: wallet(),
        min_shares: 99,
    };
    assert!(d.validate().is_ok());
    d.receiver.address = [3; 32];
    assert!(d.validate().is_err());
    let mut r = VaultRedeem {
        vault: vault(),
        shares: 99,
        receiver: wallet(),
        owner: wallet(),
        operator: wallet(),
        min_assets: 999_000,
    };
    assert!(r.validate().is_ok());
    r.min_assets = 0;
    assert!(r.validate().is_err());
    r.min_assets = 999_000;
    r.operator.address = [3; 32];
    assert!(r.validate().is_err());
}
#[test]
fn temporary_loss_remains_exposed_until_verified_redemption() {
    assert_eq!(economic_exposure(20, 980).unwrap(), 1000);
    assert!(economic_exposure(-1, 980).is_err());
    assert!(economic_exposure(i128::MAX, 1).is_err());
}
