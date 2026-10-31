use allowit::v1::prelude::*;

struct PolicyParams {}

fn new() -> PolicyParams {
    PolicyParams {}
}

async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {
    allowit::fail("Policy is not configured")
}
