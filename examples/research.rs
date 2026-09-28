use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap(ctx, "100", "USDC")?;
    cap_per_transaction(ctx, "10", "USDC")?;
    allow_actions(ctx, &["research"])?;
    require_merchant(ctx, "research.example")?;
    Ok(())
}
