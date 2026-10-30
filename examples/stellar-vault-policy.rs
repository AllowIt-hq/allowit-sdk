use allowit::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    allowit::spent_cap(grant::spent_cap)?;
    allowit::day_spent_cap(grant::day_spent_cap)?;
    allowit::day_total_cap(grant::day_total_cap)?;
    allowit::recipients(grant::recipients)?;
    allowit::tokens(grant::tokens)?;
    if ctx.amount_units == 0 {
        return Err(PolicyError {
            code: "ZERO_AMOUNT".into(),
            reason: "The deposit amount must be positive.".into(),
        });
    }
    Ok(())
}
