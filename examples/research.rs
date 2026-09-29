use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap(ctx, "100", "USDC")?;
    cap_per_transaction(ctx, "10", "USDC")?;
    require_merchant(ctx, "research.example")?;
    check_preference(
        ctx,
        "Does this purchase count as research under the user's stated purpose and definitions?",
        true,
        "85",
        true,
        "40",
    )
    .await?;
    Ok(())
}
