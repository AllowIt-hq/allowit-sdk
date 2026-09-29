use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    set_cap(ctx, "250", "USDC")?;
    let candidate = context_u64(ctx, "candidate_yield_bps")?;
    let benchmark = context_u64(ctx, "benchmark_yield_bps")?;
    if !within_percentage_points(candidate, benchmark, "1")? {
        return fail("The greener option would sacrifice more than one percentage point of return");
    }
    check_preference(ctx, "Does this transaction count as an investment under the user's stated purpose and definitions?", true, "85", true, "40").await?;
    check_preference(
        ctx,
        "Does this investment prioritize credible environmental benefits and avoid hype?",
        true,
        "80",
        false,
        "0",
    )
    .await?;
    Ok(())
}
