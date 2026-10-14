use allowit_stellar::*;
use serde_json::json;

#[test]
fn exact_quote_rejects_dust_wrong_network_and_overspend() {
    let offer = json!({"scheme":"exact","network":"stellar:testnet","asset":USDC_TESTNET,
        "amount":"10000","payTo":test_account(3),"maxTimeoutSeconds":300,
        "extra":{"areFeesSponsored":true}});
    let required = json!({"x402Version":2,"resource":{"url":"https://agent402.tools/api/hash"},"accepts":[offer]});
    assert!(select_offer(&required, "stellar:testnet", 10000).is_ok());
    assert!(select_offer(&required, "stellar:pubnet", 10000).is_err());
    assert!(select_offer(&required, "stellar:testnet", 9999).is_err());
    let mut dust = required.clone();
    dust["accepts"][0]["amount"] = json!("10001");
    assert!(select_offer(&dust, "stellar:testnet", 20000).is_err());
}

#[test]
fn urls_and_methods_are_bound_exactly() {
    assert!(validate_https_url("https://agent402.tools/api/hash?q=hi").is_ok());
    assert!(validate_https_url("http://agent402.tools/api/hash").is_err());
    assert!(validate_https_url("https://user:secret@agent402.tools/api/hash").is_err());
    assert!(validate_https_url("https://agent402.tools/api/hash#fragment").is_err());
    assert!(validate_https_url("https://127.0.0.1/api").is_err());
}

#[test]
fn official_golden_digest_and_both_credential_arms() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/golden.json")).unwrap();
    let vault = fixture["vault"].as_str().unwrap();
    let digest = authorization_digest(vault, &[8; 32], 3, 4, 172900, &[9; 32]).unwrap();
    assert_eq!(hex(&digest), fixture["customDigest"].as_str().unwrap());
    for arm in ["legacy", "v2"] {
        let entry = fixture[arm]["authorizationXdr"].as_str().unwrap();
        assert_eq!(
            hex(&host_digest("stellar:testnet", entry).unwrap()),
            fixture[arm]["hostDigest"].as_str().unwrap()
        );
    }
}

fn test_account(seed: u8) -> String {
    account_for_seed(&[seed; 32])
}
