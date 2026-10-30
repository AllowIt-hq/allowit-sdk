//! Exact SEP-56 operation values. Quotes do not prove settlement or redemption.
use crate::typed_workflow::{AccountId, AccountKind, Chain};
use alloc::string::String;
use serde::{Deserialize, Serialize};

pub const DEPOSIT_ARGUMENT_SCHEMA: &[u8] = b"allowit.stellar.sep56_deposit.v1:ScValMap(assets:i128,from:Address,min_shares:i128,operator:Address,receiver:Address,vault:Address)";
pub const REDEEM_ARGUMENT_SCHEMA: &[u8] = b"allowit.stellar.sep56_redeem.v1:ScValMap(min_assets:i128,operator:Address,owner:Address,receiver:Address,shares:i128,vault:Address)";
pub const DEPOSIT_OPERATION: &str = "stellar::sep56_deposit";
pub const REDEEM_OPERATION: &str = "stellar::sep56_redeem";

/// Economic exposure retains temporary loss until confirmed redemption.
/// Inputs must use the same authenticated denomination and integer scale.
pub fn economic_exposure(spent_expenses: i128, temporary_loss: i128) -> Result<i128, String> {
    if spent_expenses < 0 || temporary_loss < 0 {
        return Err("Vault expenses and temporary loss must be nonnegative".into());
    }
    spent_expenses
        .checked_add(temporary_loss)
        .ok_or_else(|| "Vault economic exposure overflow".into())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultDeposit {
    /// SEP-56 vault contract. The vault itself is the share token.
    pub vault: AccountId,
    /// Exact underlying base units, independent of share decimals.
    pub assets: i128,
    pub receiver: AccountId,
    pub from: AccountId,
    pub operator: AccountId,
    /// Minimum actual minted share units. Never inferred from an untrusted quote.
    pub min_shares: i128,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultRedeem {
    pub vault: AccountId,
    /// Exact share units to burn.
    pub shares: i128,
    pub receiver: AccountId,
    pub owner: AccountId,
    pub operator: AccountId,
    /// Minimum actual underlying units received.
    pub min_assets: i128,
}
fn account(account: &AccountId) -> bool {
    account.chain == Chain::Stellar
        && matches!(account.kind, AccountKind::Wallet | AccountKind::Contract)
        && account.address != [0; 32]
}
fn parties(
    vault: &AccountId,
    holder: &AccountId,
    receiver: &AccountId,
    operator: &AccountId,
) -> bool {
    account(vault)
        && vault.kind == AccountKind::Contract
        && account(holder)
        && holder == receiver
        && holder == operator
        && holder != vault
}
impl VaultDeposit {
    /// Validate owner-custody identities and exact positive bounds. This grants no authority.
    pub fn validate(&self) -> Result<(), String> {
        if !parties(&self.vault, &self.from, &self.receiver, &self.operator)
            || self.assets <= 0
            || self.min_shares <= 0
        {
            return Err("Invalid SEP-56 deposit identities or positive bounds".into());
        }
        Ok(())
    }
}
impl VaultRedeem {
    /// Validate redemption facts without signing or admitting delegated contract redemption.
    pub fn validate(&self) -> Result<(), String> {
        if !parties(&self.vault, &self.owner, &self.receiver, &self.operator)
            || self.shares <= 0
            || self.min_assets <= 0
        {
            return Err("Invalid SEP-56 redemption identities or positive bounds".into());
        }
        Ok(())
    }
}
