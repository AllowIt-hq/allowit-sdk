struct PolicyParams {}
fn new() -> PolicyParams { PolicyParams {} }
async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::require_asset(&ctx.token, "XLM")?;
    Ok(())
}
