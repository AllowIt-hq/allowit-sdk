use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn run(bytes: &[u8]) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_allowit-stellar-http"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(bytes);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/x402-vault.json")).unwrap()
}

#[test]
fn canonical_challenge_hash_uses_shared_sorted_rust_json() {
    let response = run(br#"{"op":"canonical_hash","value":{}}"#);
    assert_eq!(
        response,
        json!({"ok":true,"result":"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"})
    );
}

#[test]
fn helper_preserves_legacy_hash_inspection_and_public_state_operations() {
    let f = fixture();
    let response = run(&serde_json::to_vec(
        &json!({"op":"request_hash","request":f["prepared"]["request"]}),
    )
    .unwrap());
    assert_eq!(
        response["result"],
        "839afc064cfbdfa95f9cb50afa234c43e8be67a3ba2d8304d263145d334f63b2"
    );
    let response = run(&serde_json::to_vec(
        &json!({"op":"inspect","credential":f["signed"]["paymentSignature"]}),
    )
    .unwrap());
    assert_eq!(response["result"]["nonce"], 3);
    assert_eq!(response["result"]["vault"], f["config"]["vault"]);
    let response = run(&serde_json::to_vec(
        &json!({"op":"parse_state","resultXdr":f["stateResultXdr"]}),
    )
    .unwrap());
    assert_eq!(response["result"], f["state"]);
    let response =
        run(&serde_json::to_vec(&json!({"op":"ledger_keys","config":f["config"]})).unwrap());
    assert_eq!(response["result"].as_array().unwrap().len(), 4);
    let response = run(&serde_json::to_vec(
        &json!({"op":"validate_entries","config":f["config"],"entries":f["ledgerEntries"]}),
    )
    .unwrap());
    assert_eq!(response, json!({"ok":true,"result":null}));
}

#[test]
fn invalid_unknown_duplicate_and_oversized_input_returns_a_structured_error() {
    for bytes in [
        br#"{"op":"not-supported"}"#.as_slice(),
        br#"{"op":"inspect","credential":"invalid","surprise":true}"#.as_slice(),
        br#"{"op":"inspect","credential":"a","credential":"b"}"#.as_slice(),
        br#"{} {}"#.as_slice(),
        b"not-json".as_slice(),
    ] {
        let response = run(bytes);
        assert_eq!(response["ok"], false);
        assert!(response["error"].is_string());
        assert!(response.get("result").is_none());
    }
    let response = run(&vec![b' '; 8_388_609]);
    assert_eq!(response["ok"], false);
}
