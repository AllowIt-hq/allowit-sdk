use allowit::v1::prelude::*;

struct PolicyParams {}

fn new() -> PolicyParams { PolicyParams {} }

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::set_cap(ctx.spent_units, ctx.amount_units, &ctx.token, 5000000, "USDC", 6)?;
    if !allowit::amount_at_most(ctx.amount_units, "3")? {
        allowit::require_user_input(ctx, "Approve this payment exceeding 3 USDC?").await?;
    }
    Ok(())
}