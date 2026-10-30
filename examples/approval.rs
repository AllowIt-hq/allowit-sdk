use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    allowit::day_total_cap(50000000)?;
    if ctx.action != "research" { return Err(PolicyError { code: "ACTION_DENIED".into(), reason: "Research is required.".into() }); }
    let safety = allowit::confidence(ctx, "safety")?;
    if safety.lower_bps < 8000 {
        allowit::require_user_input(ctx, "Approve this research purchase").await?;
    }
    if ctx.amount_units > 20000000 {
        return Err(PolicyError { code: "POLICY_REJECTED".into(), reason: "Keep each purchase within 20 USDC".into() });
    }
    Ok(())
}
