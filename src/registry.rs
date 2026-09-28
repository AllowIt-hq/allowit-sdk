use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub signature: String,
    pub effect: String,
}
pub fn registry() -> Vec<FunctionInfo> {
    [
        ("set_cap", "Total spending limit", "Sets the total amount this policy can spend. Previous spending and the new request must fit within this limit.", "set_cap(ctx: &Context, amount: &str, token: &str) -> PolicyResult", "config"),
        ("cap_per_transaction", "Limit per purchase", "Rejects a purchase above this amount, even when money remains in the total allowance.", "cap_per_transaction(ctx: &Context, amount: &str, token: &str) -> PolicyResult", "context"),
        ("usdc", "USDC amount", "Writes amounts in USDC: usdc(\"25.50\")? means 25.50 USDC. Up to six decimal places, with no rounding. On Testnet this unit uses the policy's configured six-decimal test token.", "usdc(amount: &str) -> Result<u64, PolicyError>", "pure"),
        ("percent", "Percentage", "Writes a percentage from 0 to 100: percent(\"85\")? means 85%, or 8,500 basis points. Up to two decimal places, with no rounding.", "percent(value: &str) -> Result<u64, PolicyError>", "pure"),
        ("amount_at_most", "Compare purchase amount", "Checks whether the purchase is at or below this USDC amount, including equality. This comparison does not set a spending allowance.", "amount_at_most(ctx: &Context, amount: &str) -> Result<bool, PolicyError>", "context"),
        ("within_percentage_points", "Compare returns", "Checks whether the candidate return is no more than this many percentage points below the benchmark. Both returns use basis points. A higher return passes; 4% versus 5% passes a 1-point gap. This is an absolute gap, not a relative percent change.", "within_percentage_points(candidate: u64, benchmark: u64, gap: &str) -> Result<bool, PolicyError>", "pure"),
        ("allow_actions", "Permitted actions", "Allows only the listed actions. Every other action is rejected.", "allow_actions(ctx: &Context, actions: &[&str]) -> PolicyResult", "context"),
        ("require_merchant", "Required merchant", "Allows a request only when its merchant exactly matches this value.", "require_merchant(ctx: &Context, merchant: &str) -> PolicyResult", "context"),
        ("require_recipient", "Required recipient", "Allows funds to be sent only to this exact address. The executing wallet or contract verifies the destination.", "require_recipient(ctx: &Context, recipient: &str) -> PolicyResult", "context"),
        ("confidence", "Confidence interval", "Reads the supplied evidence interval. Both bounds use basis points: 10,000 means 100%. Missing or invalid evidence rejects the request.", "confidence(ctx: &Context, name: &str) -> Result<ConfidenceInterval, PolicyError>", "confidence"),
        ("semantic", "Jev preference assessment", "Reads an assessment of how the request fits this preference. A missing assessment prevents approval. A point score is not a calibrated confidence interval.", "semantic(ctx: &Context, question: &str) -> Result<ConfidenceInterval, PolicyError>", "confidence"),
        ("check_preference", "Jev preference", "Assesses this preference. Enabled approval and denial thresholds apply inclusively; all other scores ask you. Every other policy rule must still pass. The result is one preference-fit score, not calibrated confidence.", "async check_preference(ctx: &Context, question: &str, auto_approve: bool, approve_percent: &str, auto_deny: bool, deny_percent: &str) -> PolicyResult", "user_input"),
        ("context_u64", "Request value", "Reads a whole-number value supplied with the request, such as an expected return in basis points. Missing or non-whole values reject the request. Supplied facts need a trustworthy source.", "context_u64(ctx: &Context, key: &str) -> Result<u64, PolicyError>", "context"),
        ("require_user_input", "Ask for approval", "Pauses for your approval. Declining rejects the request.", "async require_user_input(ctx: &Context, prompt: &str) -> PolicyResult", "user_input"),
        ("fail", "Reject the request", "Stops evaluation and rejects the request with this explanation. No spending is authorized.", "fail(reason: &str) -> PolicyResult", "pure"),
    ].into_iter().map(|(name,title,description,signature,effect)| FunctionInfo { name:name.into(), title:title.into(), description:description.into(), signature:signature.into(), effect:effect.into() }).collect()
}

#[cfg(feature = "compiler")]
pub(crate) fn function(name: &str) -> Option<FunctionInfo> {
    registry().into_iter().find(|f| f.name == name)
}
