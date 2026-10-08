use allowit::v1::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::set_cap(ctx, "100", "USDC")?;
    allowit::cap_per_transaction(ctx, "100", "USDC")?;
    allowit::require_recipient(ctx, "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB")?;
    if ctx.amount_units != allowit::usdc("100")? {
        return allowit::fail("Transaction amount must be exactly 100 USDC");
    }
    Ok(())
}