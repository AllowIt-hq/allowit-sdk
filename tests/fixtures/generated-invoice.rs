use allowit::v1::prelude::*;

struct PolicyParams {}

fn new() -> PolicyParams { PolicyParams {} }

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::set_cap(ctx.spent_units, ctx.amount_units, &ctx.token, 100000000, "USDC", 6)?;
    allowit::cap_per_transaction(ctx.amount_units, &ctx.token, 100000000, "USDC", 6)?;
    allowit::require_recipient(&ctx.recipient, "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB")?;
    if ctx.amount_units != allowit::usdc("100")? {
        return allowit::fail("Transaction amount must be exactly 100 USDC");
    }
    Ok(())
}