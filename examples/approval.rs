use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap(ctx, "50", "USDC")?;
    allow_actions(ctx, &["research"])?;
    let safety = confidence(ctx, "safety")?;
    if safety.lower_bps < 8000 {
        require_user_input(ctx, "Approve this research purchase").await?;
    }
    if ctx.amount_units > 20000000 {
        return fail("Keep each purchase within 20 USDC");
    }
    Ok(())
}
