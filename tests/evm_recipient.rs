use allowit_sdk::{
    Context, PolicyRuleFacts, TrustedPolicyState, compile, evaluate_with_policy_rules,
};

// The EIP-55 form an owner pastes into a prompt, and the lowercase form the
// Tempo host passes as ctx.recipient after parsing the account.
const CHECKSUMMED: &str = "0x33b8aF346B1e9462a1EC79866a19f03bB8351Ff4";
const LOWERCASE: &str = "0x33b8af346b1e9462a1ec79866a19f03bb8351ff4";
const SOLANA: &str = "GmaDrppBC7P5ARKV8g3djiwP89vz1jLK23V2GBjuAEGB";

fn source(recipient: &str) -> String {
    format!(
        "use allowit::prelude::*; async fn _execute(ctx:&Context)->PolicyResult {{ allowit::recipients(&[{recipient:?}])?; Ok(()) }}"
    )
}
fn context(recipient: &str, network: &str) -> Context {
    let mut c: Context = serde_json::from_str(include_str!("../examples/context.json")).unwrap();
    c.network = network.into();
    c.recipient = recipient.into();
    c.policy_state = Some(TrustedPolicyState::default());
    c.rule_facts = Some(PolicyRuleFacts {
        operation_id: "recipient-op".into(),
        grant_available: true,
        amount_units: 300_000,
        asset_id: "AlphaUSD".into(),
        recipient: recipient.into(),
        now: 100,
        expense: None,
    });
    c
}
fn tempo_decision(literal: &str, recipient: &str) -> allowit_sdk::Decision {
    evaluate_with_policy_rules(
        &compile(&source(literal)).unwrap(),
        &context(recipient, "tempo:testnet"),
    )
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
            "ADDRESS_NOT_ALLOWED",
            "{recipient}"
        );
    }
}

#[test]
fn solana_recipients_stay_case_sensitive() {
    let policy = compile(&source(SOLANA)).unwrap();
    assert_eq!(
        evaluate_with_policy_rules(&policy, &context(SOLANA, "solana:devnet")).outcome,
        "pass"
    );
    for recipient in [SOLANA.to_ascii_lowercase(), SOLANA.to_ascii_uppercase()] {
        assert_eq!(
            evaluate_with_policy_rules(&policy, &context(&recipient, "solana:devnet")).code,
            "ADDRESS_NOT_ALLOWED"
        );
    }
}
#[test]
fn direct_facade_has_no_authenticated_host_and_fails_closed() {
    assert_eq!(
        allowit_sdk::recipients(&[CHECKSUMMED]).unwrap_err().code,
        "POLICY_CONTEXT_REQUIRED"
    );
}
