use allowit::prelude::*;

async fn _execute(ctx: &Context) -> PolicyResult {
    return Err(PolicyError {
        code: "POLICY_NOT_CONFIGURED".into(),
        reason: "Configure the policy before execution.".into(),
    });
}
