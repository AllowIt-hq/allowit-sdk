use allowit::prelude::*;

pub async fn evaluate(ctx: &Context) -> PolicyResult {
    allowit::day_total_cap(100000000)?;
    allowit::transaction_cap(10000000)?;
    if ctx.merchant != "research.example" { return Err(PolicyError { code: "MERCHANT_DENIED".into(), reason: "Merchant is not allowed.".into() }); }
    allowit::prelude::check_preference(
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
