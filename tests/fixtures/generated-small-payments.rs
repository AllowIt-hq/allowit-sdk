use allowit::v1::prelude::*;

struct PolicyParams {}

fn new() -> PolicyParams { PolicyParams {} }

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::set_cap_per_day(ctx.daily_spent_units, ctx.spent_day, ctx.now, ctx.amount_units, &ctx.token, 5000000, "USDC", 6)?;
    allowit::cap_per_transaction(ctx.amount_units, &ctx.token, 2000000, "USDC", 6)?;
    Ok(())
}