use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap(ctx, "250", "USDC")?;
    allow_actions(ctx, &["investment"])?;
    let candidate = context_u64(ctx, "candidate_yield_bps")?;
    let benchmark = context_u64(ctx, "benchmark_yield_bps")?;
    if !within_percentage_points(candidate, benchmark, "1")? {
        return fail("The greener option would sacrifice more than one percentage point of return");
    }
    let preference = semantic(ctx, "Does this investment prioritize credible environmental benefits and avoid hype?")?;
    if preference.lower_bps < percent("80")? {
        require_user_input(ctx, "This investment may not fit your environmental preferences. Approve it?").await?;
    }
    Ok(())
}
