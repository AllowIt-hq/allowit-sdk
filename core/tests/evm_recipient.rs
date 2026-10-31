use allowit_sdk::{
    Context, NativePolicyStorage, Profile, ServicePaymentAsset, compile, evaluate,
    evaluate_with_trace_native_payment,
};

// The EIP-55 form an owner pastes into a prompt, and the lowercase form the
// Tempo host passes as ctx.recipient after parsing the account.
const CHECKSUMMED: &str = "0x33b8aF346B1e9462a1EC79866a19f03bB8351Ff4";
const LOWERCASE: &str = "0x33b8af346b1e9462a1ec79866a19f03bb8351ff4";
const SOLANA: &str = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";

fn tempo_source(recipient: &str) -> String {
    format!(
        r#"use allowit::v1::prelude::*;
struct PolicyParams {{}} fn new() -> PolicyParams {{ PolicyParams {{}} }}
async fn _execute(ctx: &Context, params: &PolicyParams) -> PolicyResult {{
    allowit::require_asset(&ctx.token, "AlphaUSD")?;
    allowit::cap_per_transaction(ctx.amount_units, &ctx.token, 5_000_000, "AlphaUSD", 6)?;
    allowit::require_recipient(&ctx.recipient, {recipient:?})?;
    Ok(())
}}"#
    )
}
fn tempo_context(recipient: &str) -> Context {
    let mut ctx: Context =
        serde_json::from_str(include_str!("../../docs/examples/context.json")).unwrap();
    ctx.network = "tempo:testnet".into();
    ctx.token = "AlphaUSD".into();
    ctx.recipient = recipient.into();
    ctx.amount_units = 300_000;
    ctx.native_policy_storage = Some(NativePolicyStorage {
        daily_limit_units: 5_000_000,
        action_limit_units: 5_000_000,
    });
    ctx
}
fn tempo_decision(literal: &str, recipient: &str) -> allowit_sdk::Decision {
    let policy = compile(&tempo_source(literal)).unwrap();
    let asset = ServicePaymentAsset {
        network: "tempo:testnet".into(),
        asset: "AlphaUSD".into(),
        decimals: 6,
    };
    evaluate_with_trace_native_payment(&policy, &tempo_context(recipient), &asset).0
}

#[test]
fn tempo_checksummed_recipient_literal_pays_the_lowercase_host_recipient() {
    for (literal, recipient) in [
        (CHECKSUMMED, LOWERCASE),
        (CHECKSUMMED, CHECKSUMMED),
        (LOWERCASE, CHECKSUMMED),
        (
            LOWERCASE,
            &LOWERCASE.to_ascii_uppercase().replacen("0X", "0x", 1),
        ),
    ] {
        let decision = tempo_decision(literal, recipient);
        assert_eq!(
            decision.outcome, "pass",
            "{literal} vs {recipient}: {decision:?}"
        );
    }
}

#[test]
fn tempo_recipient_still_rejects_a_different_or_malformed_address() {
    let one_digit_off = LOWERCASE.replacen("ff4", "ff5", 1);
    for recipient in [
        one_digit_off.as_str(),
        "0x0000000000000000000000000000000000000001",
        // Only exact `0x` + 40-hex values compare case-insensitively.
        &LOWERCASE[..41],
        &LOWERCASE.replacen("0x", "0X", 1),
        &format!("{LOWERCASE}0"),
    ] {
        assert_eq!(
            tempo_decision(CHECKSUMMED, recipient).code,
            "RECIPIENT_NOT_ALLOWED",
            "{recipient}"
        );
    }
}

#[test]
fn solana_recipients_stay_case_sensitive() {
    let source = format!(
        "pub async fn evaluate(ctx: &Context) -> PolicyResult {{ require_recipient(ctx, {SOLANA:?})?; Ok(()) }}"
    );
    let policy = compile(&source).unwrap();
    let mut ctx: Context =
        serde_json::from_str(include_str!("../../docs/examples/context.json")).unwrap();
    ctx.recipient = SOLANA.into();
    assert_eq!(evaluate(&policy, Profile::Oracle, &ctx).outcome, "pass");
    for recipient in [SOLANA.to_ascii_lowercase(), SOLANA.to_ascii_uppercase()] {
        ctx.recipient = recipient;
        assert_eq!(
            evaluate(&policy, Profile::Oracle, &ctx).code,
            "RECIPIENT_NOT_ALLOWED"
        );
    }
}

#[test]
fn native_primitive_matches_evm_case_insensitively_and_solana_exactly() {
    assert!(allowit_sdk::require_recipient(LOWERCASE, CHECKSUMMED).is_ok());
    assert!(allowit_sdk::require_recipient(CHECKSUMMED, LOWERCASE).is_ok());
    assert_eq!(
        allowit_sdk::require_recipient(&LOWERCASE.replacen("ff4", "ff5", 1), CHECKSUMMED)
            .unwrap_err()
            .code,
        "RECIPIENT_NOT_ALLOWED"
    );
    assert!(allowit_sdk::require_recipient(SOLANA, SOLANA).is_ok());
    assert!(allowit_sdk::require_recipient(&SOLANA.to_ascii_lowercase(), SOLANA).is_err());
}
