use allowit::v1::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::allow_actions(ctx, &["stellar::sep56_deposit"])?;
    if !allowit::is_one_of(
        &ctx.recipient,
        &[
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4",
            "CBNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVWW23LNNVXQOK",
        ],
    )? {
        return allowit::fail("Vault destination is not allowed.");
    }
    Ok(())
}

async fn _server_execute(ctx: &Context) -> PolicyResult {
    // Authenticated expenses plus temporary loss, in installed underlying base units.
    let exposure = allowit::context_u64(ctx, "vault_economic_exposure_units")?;
    if exposure > 1_000_000_000 {
        return allowit::fail("Vault exposure exceeds the owner limit.");
    }
    Ok(())
}
