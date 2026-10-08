use allowit::v1::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::set_cap(ctx, "5", "USDC")?;
    if !allowit::amount_at_most(ctx, "3")? {
        allowit::require_user_input(ctx, "Approve this payment exceeding 3 USDC?").await?;
    }
    Ok(())
}