use allowit::v1::prelude::*;

struct PolicyParams {
    daily_limit: OwnerLimit,
    action_limit: OwnerLimit,
}

fn new() -> PolicyParams {
    PolicyParams {
        daily_limit: allowit::owner_limit("native_daily_limit", 5_000_000),
        action_limit: allowit::owner_limit("native_action_limit", 1_000_000),
    }
}

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    if ctx.amount_units > allowit::stored_limit(ctx, params.action_limit)? {
        return allowit::fail("Payment exceeds the current owner limit");
    }
    if ctx.spent_units + ctx.amount_units > allowit::stored_limit(ctx, params.daily_limit)? {
        return allowit::fail("Daily spending exceeds the current owner limit");
    }
    Ok(())
}
