use allowit::v1::prelude::*;

struct PolicyParams {
    total_limit: u64,
    recipients: &'static [&'static str],
    enabled: bool,
    currency: &'static str,
}

fn new() -> PolicyParams {
    PolicyParams {
        total_limit: 5_000_000,
        recipients: &["11111111111111111111111111111111"],
        enabled: true,
        currency: "USDC",
    }
}

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::set_cap(ctx.spent_units, ctx.amount_units, &ctx.token, params.total_limit, params.currency, 6)?;
    if !params.enabled {
        return allowit::fail("Policy is disabled");
    }
    if !allowit::is_one_of(&ctx.recipient, params.recipients)? {
        return allowit::fail("Recipient is not allowed");
    }
    allowit::cap_per_transaction(ctx.amount_units, &ctx.token, allowit::usdc("2")?, "USDC", 6)?;
    Ok(())
}
