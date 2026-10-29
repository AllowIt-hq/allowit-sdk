use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub signature: String,
    pub effect: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typed_signature: Option<crate::typed_workflow::WorkflowSignature>,
}
pub fn registry() -> Vec<FunctionInfo> {
    let functions: Vec<FunctionInfo> = [
        ("set_cap_per_day", "Daily spending limit", "Sets the amount this policy can spend per UTC day. Authenticated daily spending and the new request must fit within this limit.", "set_cap_per_day(ctx: &Context, amount: &str, token: &str) -> PolicyResult", "config"),
        ("cap_per_transaction", "Limit per purchase", "Rejects a purchase above this amount, even when money remains in the total allowance.", "cap_per_transaction(ctx: &Context, amount: &str, token: &str) -> PolicyResult", "context"),
        ("cap_purchase_tiers", "Purchase tiers", "Limits expensive purchases: the first price band allows the stated count. Each cheaper band halves the ceiling and doubles the count. No minimum purchase amount; available funding still limits spending. Uses authoritative purchase history, including reservations.", "cap_purchase_tiers(ctx: &Context, maximum: &str, first_count: u64, token: &str) -> PolicyResult", "ledger"),
        ("usdc", "USDC amount", "Writes amounts in USDC: usdc(\"25.50\")? means 25.50 USDC. Up to six decimal places, with no rounding. On Testnet this unit uses the policy's configured six-decimal test token.", "usdc(amount: &str) -> Result<u64, PolicyError>", "pure"),
        ("percent", "Percentage", "Writes a percentage from 0 to 100: percent(\"85\")? means 85%, or 8,500 basis points. Up to two decimal places, with no rounding.", "percent(value: &str) -> Result<u64, PolicyError>", "pure"),
        ("amount_at_most", "Compare purchase amount", "Checks whether the purchase is at or below this USDC amount, including equality. This comparison does not set a spending allowance.", "amount_at_most(ctx: &Context, amount: &str) -> Result<bool, PolicyError>", "context"),
        ("within_percentage_points", "Compare returns", "Checks whether the candidate return is no more than this many percentage points below the benchmark. Both returns use basis points. A higher return passes; 4% versus 5% passes a 1-point gap. This is an absolute gap, not a relative percent change.", "within_percentage_points(candidate: u64, benchmark: u64, gap: &str) -> Result<bool, PolicyError>", "pure"),
        ("allow_actions", "Exact action labels", "Compares the caller-supplied action field with literal strings. It does not verify an action's category or purpose; use check_preference for classification.", "allow_actions(ctx: &Context, actions: &[&str]) -> PolicyResult", "context"),
        ("require_asset", "Payment asset", "Binds the authenticated payment asset without setting a spending cap.", "require_asset(ctx: &Context, asset: &str) -> PolicyResult", "config"),
        ("require_merchant", "Required merchant", "Allows a request only when its merchant exactly matches this value.", "require_merchant(ctx: &Context, merchant: &str) -> PolicyResult", "context"),
        ("require_recipient", "Required recipient", "Allows funds to be sent only to this exact address. The executing wallet or contract verifies the destination.", "require_recipient(ctx: &Context, recipient: &str) -> PolicyResult", "context"),
        ("confidence", "Confidence interval", "Reads the supplied evidence interval. Both bounds use basis points: 10,000 means 100%. Missing or invalid evidence rejects the request.", "confidence(ctx: &Context, name: &str) -> Result<ConfidenceInterval, PolicyError>", "confidence"),
        ("preference_evidence", "Preference evidence", "Reads the named trusted optional numeric evidence. Use only inside the matching preference guard.", "preference_evidence(ctx: &Context, question: &str) -> Option<ConfidenceInterval>", "confidence"),
        ("semantic", "Preference assessment", "Reads an assessment of how the request fits this preference. A missing assessment prevents approval. A point score is not a calibrated confidence interval.", "semantic(ctx: &Context, question: &str) -> Result<ConfidenceInterval, PolicyError>", "confidence"),
        ("check_preference", "Preference", "Assesses this preference. Enabled approval and denial thresholds apply inclusively; all other scores ask you. Every other policy rule must still pass. The result is one preference-fit score, not calibrated confidence.", "async check_preference(ctx: &Context, question: &str, deny: Threshold, approve: Threshold) -> PolicyResult", "user_input"),
        ("context_u64", "Request value", "Reads a whole-number value supplied with the request, such as an expected return in basis points. Missing or non-whole values reject the request. Supplied facts need a trustworthy source.", "context_u64(ctx: &Context, key: &str) -> Result<u64, PolicyError>", "context"),
        ("require_user_input", "Ask for approval", "Pauses for your approval. Declining rejects the request.", "async require_user_input(ctx: &Context, prompt: &str) -> PolicyResult", "user_input"),
        ("owner_limit", "Owner-controlled limit", "Declares one named native limit and its initial integer units in new(). Owner authorization controls later native account updates.", "owner_limit(key: &'static str, initial_units: u64) -> OwnerLimit", "storage_declaration"),
        ("spent", "Copy named spent values", "Returns the current named execution values without changing Grant.", "spent(state: &State) -> Result<Spent, PolicyError>", "native_state"),
        ("with_spent", "Replace spent values", "Returns state with exactly the supplied spent map and the same Grant.", "with_spent(state: &State, spent: Spent) -> Result<State, PolicyError>", "native_state"),
        ("state_get", "Read policy property", "Reads one named scalar from trusted native storage. Missing keys use the source default.", "state_get(spent: &Spent, name: &str, default: Scalar) -> Result<Scalar, PolicyError>", "native_state"),
        ("grant_get", "Read owner value", "Reads one named scalar from the owner Grant without changing it.", "grant_get(grant: &Grant, name: &str, default: Scalar) -> Result<Scalar, PolicyError>", "native_state"),
        ("state_set", "Write policy property", "Writes one named scalar through the native policy capability.", "state_set(spent: Spent, name: &str, value: Scalar) -> Result<Spent, PolicyError>", "native_state"),
        ("state_remove", "Remove policy property", "Removes one named property through the native policy capability.", "state_remove(spent: Spent, name: &str) -> Result<Spent, PolicyError>", "native_state"),
        ("stored_limit", "Read native policy storage", "Reads a declared limit from verified current native account state. Missing trusted state prevents approval.", "stored_limit(ctx: &Context, limit: OwnerLimit) -> Result<u64, PolicyError>", "native_storage"),
        ("is_one_of", "List membership", "Checks whether the primitive string equals an entry in a bounded inline or constructor list.", "is_one_of(value: &str, allowed: &[&str]) -> Result<bool, PolicyError>", "pure"),
        ("fail", "Reject the request", "Stops evaluation and rejects the request with this explanation. No spending is authorized.", "fail(reason: &str) -> PolicyResult", "pure"),
    ].into_iter().map(|(name,title,description,signature,effect)| FunctionInfo { name:name.into(), title:title.into(), description:description.into(), signature:signature.into(), effect:effect.into(), typed_signature:None }).collect();
    let mut result = functions.clone();
    for function in functions {
        let mut qualified = function.clone();
        qualified.name = alloc::format!("allowit::{}", function.name);
        qualified.signature = match function.name.as_str() {
            "check_preference"=>"async allowit::check_preference(evidence: Option<ConfidenceInterval>, question: &str, deny: Threshold, approve: Threshold) -> PolicyResult".into(),
            "set_cap_per_day"=>"allowit::set_cap_per_day(daily_spent_units: Option<u64>, spent_day: Option<u64>, now: u64, amount_units: u64, token: &str, limit_units: u64, currency: &str, decimals: u64) -> PolicyResult".into(),
            "cap_per_transaction"=>"allowit::cap_per_transaction(amount_units: u64, token: &str, limit_units: u64, currency: &str, decimals: u64) -> PolicyResult".into(),
            "cap_purchase_tiers"=>"allowit::cap_purchase_tiers(amount_units: u64, token: &str, purchase_counts: &Option<Vec<u64>>, maximum_units: u64, first_count: u64, currency: &str, decimals: u64) -> PolicyResult".into(),
            "allow_actions"=>"allowit::allow_actions(action: &str, actions: &[&str]) -> PolicyResult".into(),
            "require_asset"=>"allowit::require_asset(token: &str, asset: &str) -> PolicyResult".into(),
            "require_merchant"=>"allowit::require_merchant(merchant: &str, required: &str) -> PolicyResult".into(),
            "require_recipient"=>"allowit::require_recipient(recipient: &str, required: &str) -> PolicyResult".into(),
            "amount_at_most"=>"allowit::amount_at_most(amount_units: u64, amount: &str) -> Result<bool, PolicyError>".into(),
            _=>function.signature.replace(&function.name,&qualified.name)
        };
        result.push(qualified);
    }
    for method in [
        "daily_cap",
        "transaction_cap",
        "total_cap",
        "call_cap",
        "recipient",
        "expiry",
        "token",
    ] {
        for prefix in ["", "allowit::", "engine::", "self::"] {
            result.push(FunctionInfo{name:alloc::format!("{prefix}{method}"),title:"Selected policy rule".into(),description:"Returns updated spent from one selected rule. Placement is fixed during authoring.".into(),signature:alloc::format!("{prefix}{method}(request: &Context, state: &State) -> Result<Spent, PolicyError>"),effect:"native_rule".into(),typed_signature:None});
        }
    }
    result.push(FunctionInfo {
        name: "allowit::service_call".into(), title: "Paid service call".into(),
        description: "Admits one call using authenticated run input and exact payment, swap and service-fee ceilings. True means admitted plan, not payment or delivery. The host settles through the selected native wallet and service adapter after every policy guard passes.".into(),
        signature: "allowit::service_call(service_id: &str, input_key: &str, max_payment_units: u64, max_rail_debit_units: u64, max_service_fee_units: u64) -> bool".into(),
        effect: "service_call".into(), typed_signature:None,
    });
    for (name, title, description, signature) in [
        (
            "allowit::execution_request_validate",
            "Validate native request",
            "Checks the complete request through the installed native profile. Structural validity is not spending approval.",
            "allowit::execution_request_validate(request: &ExecutionRequest) -> Result<ValidatedExecutionRequest>",
        ),
        (
            "allowit::payment_request_from_curl",
            "Convert payment challenge",
            "Converts an authenticated bound challenge into unsigned payment facts. None means a valid bound free response; failures remain errors.",
            "allowit::payment_request_from_curl(outcome: &CurlOutcome, request: &CurlRequest) -> Result<Option<PaymentRequest>>",
        ),
    ] {
        result.push(FunctionInfo {
            name: name.into(),
            title: title.into(),
            description: description.into(),
            signature: signature.into(),
            effect: "pure_validation".into(),
            typed_signature: crate::typed_workflow::signature(name),
        });
    }
    for (name, ty) in [
        (
            "allowit::execution_request_cap",
            "ValidatedExecutionRequest",
        ),
        ("allowit::payment_request_cap", "PaymentRequest"),
    ] {
        result.push(FunctionInfo{name:name.into(),title:"Typed asset budget".into(),description:"Checks complete same-asset debit and cumulative fee bounds against an owner-authored total budget and protected prior consumption. Other assets need separate guards. The host atomically reserves the approved plan before signing.".into(),signature:alloc::format!("{name}(request: &{ty}, budget_id: &str, asset_id: &str, decimals: u64, total_budget_units: u64, max_debit_units: u64, max_fee_units: u64) -> PolicyResult"),effect:"budget_guard".into(),typed_signature:crate::typed_workflow::signature(name)});
    }
    result.push(FunctionInfo {
        name: "stellar::sep41_transfer".into(),
        title: "Stellar token transfer".into(),
        description: "Transfers the exact bound asset and native amount through the installed Stellar execute wrapper after its private policy approves. The compiler-bound Stellar source profile admits this effect; it is not a callable pure policy function.".into(),
        signature: "stellar::sep41_transfer(token: AccountId, from: AccountId, to: AccountId, amount: i128) -> Result<TransferReceipt>".into(),
        effect: "native_effect".into(),
        typed_signature: None,
    });
    result
}

#[cfg(feature = "compiler")]
pub(crate) fn function(name: &str) -> Option<FunctionInfo> {
    registry().into_iter().find(|f| f.name == name)
}

/// Resolve only registered source spellings to the existing policy operation.
/// Vendor operations need their own implementation before registration.
#[cfg(feature = "compiler")]
pub(crate) fn canonical_function(name: &str) -> Option<String> {
    if function(name)?.effect == "native_effect" {
        return None;
    }
    if name.starts_with("self::")
        || name.starts_with("engine::")
        || name == "allowit::service_call"
        || crate::typed_workflow::signature(name).is_some()
    {
        return Some(name.into());
    }
    Some(name.rsplit("::").next()?.into())
}
