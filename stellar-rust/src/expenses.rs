//! Signed conservative accounting ceilings for supported native effects.
use crate::*;
pub use allowit_policy_sdk::typed_workflow::ExpenseBounds;

pub(crate) fn validate(
    c: &WalletConfig,
    request: &ExecutionRequest,
    operation: &StellarOperation,
) -> Result<()> {
    let Some(e) = &request.expense else {
        return Ok(());
    };
    if workflow_address(&e.asset)? != c.asset
        || e.asset.kind != AccountKind::Contract
        || e.decimals != 7
        || e.gas_units < 0
        || e.conversion_units < 0
        || e.spent_units < 0
        || e.observed_ledger == 0
    {
        return Err("Expense identity, units or observation is invalid".into());
    }
    if e.max_fee_stroops == 0 {
        if e.gas_units != 0 {
            return Err("Unattributed sponsor gas cannot be charged as policy expense".into());
        }
    } else if c.asset_symbol != "XLM" || c.asset_issuer.is_some() {
        return Err(
            "Expense gas conversion requires an authenticated accounting-asset quote".into(),
        );
    } else if e.gas_units != i128::from(e.max_fee_stroops) {
        return Err("XLM expense gas must equal its exact attributable native ceiling".into());
    }
    let expected = match operation {
        StellarOperation::Transfer(t) => {
            if e.conversion_units != 0 {
                return Err("A payment cannot claim swap conversion expenses".into());
            }
            t.amount.checked_add(e.gas_units)
        }
        StellarOperation::VaultDeposit(d) => {
            if e.conversion_units > d.assets {
                return Err("Deposit conversion ceiling exceeds its gross assets".into());
            }
            e.conversion_units.checked_add(e.gas_units)
        }
    }
    .ok_or("Expense total overflow")?;
    if e.spent_units != expected {
        return Err("Expense amount does not match the supported native operation".into());
    }
    operation
        .movement()
        .amount
        .checked_add(e.gas_units)
        .ok_or("Gross daily amount overflow")?;
    Ok(())
}
pub(crate) fn value(
    c: &WalletConfig,
    request: &ExecutionRequest,
    operation: &StellarOperation,
) -> Result<ScVal> {
    validate(c, request, operation)?;
    let Some(e) = &request.expense else {
        return Ok(ScVal::Void);
    };
    let native = value_map(vec![
        ("asset", ScVal::Address(address(&c.asset)?)),
        ("decimals", ScVal::U32(e.decimals)),
        ("gas_units", i128_value(e.gas_units)),
        ("conversion_units", i128_value(e.conversion_units)),
        ("spent_units", i128_value(e.spent_units)),
        ("max_fee_stroops", ScVal::U32(e.max_fee_stroops)),
        ("observed_ledger", ScVal::U32(e.observed_ledger)),
        ("day", ScVal::U64(e.day)),
    ])?;
    value_bytes(&native.to_xdr(limits()).map_err(err)?)
}
