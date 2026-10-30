use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    allowit::day_total_cap(250000000)?;
    let candidate = allowit::context_u64(ctx, "candidate_yield_bps")?;
    let benchmark = allowit::context_u64(ctx, "benchmark_yield_bps")?;
    if !allowit::within_percentage_points(candidate, benchmark, "1")? {
        return Err(PolicyError { code: "POLICY_REJECTED".into(), reason: "The greener option would sacrifice more than one percentage point of return".into() });
    }
    allowit::prelude::check_preference(ctx, "Does this transaction count as an investment under the user's stated purpose and definitions?", true, "85", true, "40").await?;
    allowit::prelude::check_preference(
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
