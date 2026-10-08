use allowit::v1::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::set_cap(ctx, "5", "USDC")?;
    allowit::cap_per_transaction(ctx, "2", "USDC")?;
    Ok(())
}