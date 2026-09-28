use allowit_contract_core::{Artifact, CORE_VERSION, Mandate, Request, State};

pub const SIMPLE: &str = "pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap(ctx, \"100\", \"USDC\")?; cap_per_transaction(ctx, \"10\", \"USDC\")?; allow_actions(ctx, &[\"research\"])?; Ok(()) }";
pub const INPUT: &str = "pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap(ctx, \"100\", \"USDC\")?; require_user_input(ctx, \"Approve this purchase?\").await?; Ok(()) }";

pub fn fixture(source: &str) -> State {
    let compiled = allowit_sdk::compile(source).unwrap();
    let artifact = Artifact {
        original_intent: "Allow research purchases up to 10 USDC each.".into(),
        source_hash: compiled.source_hash.clone(),
        ir_hash: compiled.ir_hash.clone(),
        registry_version: compiled.registry_version.clone(),
        core_version: CORE_VERSION.into(),
        compiler_version: "0.1.0".into(),
        ir: compiled.ir,
    };
    let bytes = serde_json::to_vec(&artifact).unwrap();
    let mandate = Mandate {
        policy_id: [9; 32],
        owner: [1; 32],
        executor: [2; 32],
        compiler: [3; 32],
        compiler_key_id: "test-key-v1".into(),
        compiler_version: "0.1.0".into(),
        evidence_authority: None,
        registry_version: compiled.registry_version,
        core_version: CORE_VERSION.into(),
        network: "devnet".into(),
        asset: [4; 32],
        asset_decimals: 6,
        recipient: [5; 32],
        recipient_address: "recipient".into(),
        action: "research".into(),
        merchant: "Example merchant".into(),
        revision: 1,
        expires_at: 2000,
        allocation_units: 100_000_000,
        source_hash: compiled.source_hash,
        ir_hash: compiled.ir_hash,
        artifact_hash: allowit_sdk::digest(&bytes),
    };
    State {
        mandate,
        artifact: bytes,
        active: true,
        revoked: false,
        spent_units: 0,
        next_nonce: 0,
    }
}

pub fn request(state: &State, amount: u64) -> Request {
    let m = &state.mandate;
    Request {
        nonce: state.next_nonce,
        revision: m.revision,
        amount_units: amount,
        asset: m.asset,
        recipient: m.recipient,
        network: m.network.clone(),
        action: m.action.clone(),
        merchant: m.merchant.clone(),
        source_hash: m.source_hash.clone(),
        ir_hash: m.ir_hash.clone(),
        evidence: None,
        runtime_context: "{}".into(),
    }
}

pub const CONFIDENCE: &str = "pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap(ctx, \"100\", \"USDC\")?; let interval = confidence(ctx, \"merchant\")?; if interval.lower_bps < 9000 { require_user_input(ctx, \"Approve the uncertain merchant?\").await?; } Ok(()) }";

pub const SEMANTIC_QUESTION: &str = "Does this purchase satisfy the owner intent?";
pub const SEMANTIC: &str = "pub async fn evaluate(ctx: &Context) -> PolicyResult { set_cap(ctx, \"100\", \"USDC\")?; let risk = context_u64(ctx, \"risk\")?; if risk > 10 { return fail(\"Risk is too high.\"); } let interval = semantic(ctx, \"Does this purchase satisfy the owner intent?\")?; if interval.lower_bps < 9000 { require_user_input(ctx, \"Approve the uncertain request?\").await?; } Ok(()) }";

pub fn maximum_semantic_fixture() -> State {
    let nested = "if (((((ctx.amount_units + 0) + 0) + 0) + 0) > 0) { cap_per_transaction(ctx, \"10\", \"USDC\")?; }";
    let mut body: alloc::string::String = nested.into();
    let mut best = None;
    loop {
        let source = SEMANTIC.replace("Ok(()) }", &alloc::format!("{body} Ok(()) }}"));
        let mut state = fixture(&source);
        let mut artifact: Artifact = serde_json::from_slice(&state.artifact).unwrap();
        if state.artifact.len() > allowit_contract_core::MAX_CHAIN_ARTIFACT_BYTES {
            break;
        }
        let padding = (allowit_contract_core::MAX_CHAIN_ARTIFACT_BYTES - state.artifact.len())
            .min(2048 - artifact.original_intent.len());
        artifact.original_intent.push_str(&"x".repeat(padding));
        state.artifact = serde_json::to_vec(&artifact).unwrap();
        state.mandate.artifact_hash = allowit_sdk::digest(&state.artifact);
        allowit_contract_core::validate_chain_artifact(&state.mandate, &state.artifact).unwrap();
        best = Some(state);
        body += " cap_per_transaction(ctx, \"10\", \"USDC\")?;";
    }
    let state = best.unwrap();
    assert_eq!(
        state.artifact.len(),
        allowit_contract_core::MAX_CHAIN_ARTIFACT_BYTES
    );
    state
}

pub fn semantic_evidence(state: &State, request: &mut Request, lower: u64) {
    evidence(state, request, lower, 9800);
    request.evidence.as_mut().unwrap().intervals[0].name =
        allowit_sdk::digest(SEMANTIC_QUESTION.as_bytes());
}

pub fn evidence(state: &State, request: &mut Request, lower: u64, upper: u64) {
    use allowit_contract_core::{Evidence, Interval, request_hash};
    let authority = state.mandate.evidence_authority.as_ref().unwrap();
    request.evidence = Some(Evidence {
        request_hash: request_hash(&state.mandate, request).unwrap(),
        key_id: authority.key_id.clone(),
        version: authority.version.clone(),
        issued_at: 990,
        expires_at: 1100,
        intervals: vec![Interval {
            name: "merchant".into(),
            lower_bps: lower,
            upper_bps: upper,
        }],
    });
}
extern crate alloc;
use alloc::vec;
