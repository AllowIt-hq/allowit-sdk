use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn standalone_cli_and_lsp_transport_run_without_ui() {
    let output = Command::new(env!("CARGO_BIN_EXE_allowit"))
        .args([
            "evaluate",
            "examples/research.rs",
            "examples/context.json",
            "oracle",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["decision"]["code"], "SEMANTIC_EVIDENCE_REQUIRED");
    let mut child = Command::new(env!("CARGO_BIN_EXE_allowit"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let source = include_str!("../examples/research.rs");
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///policy.rs","text":source}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"allowit/workflow","params":{"textDocument":{"uri":"file:///policy.rs"}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"shutdown"}),
        json!({"jsonrpc":"2.0","method":"exit"}),
    ];
    {
        let input = child.stdin.as_mut().unwrap();
        for message in messages {
            let bytes = serde_json::to_vec(&message).unwrap();
            write!(input, "Content-Length: {}\r\n\r\n", bytes.len()).unwrap();
            input.write_all(&bytes).unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let mut remaining = output.stdout.as_slice();
    let mut responses = vec![];
    while !remaining.is_empty() {
        let end = remaining.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let header = std::str::from_utf8(&remaining[..end]).unwrap();
        let length = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        responses
            .push(serde_json::from_slice::<Value>(&remaining[end + 4..end + 4 + length]).unwrap());
        remaining = &remaining[end + 4 + length..];
    }
    assert_eq!(responses.len(), 4);
    assert_eq!(
        responses[2]["result"]["policy"]["language"],
        "allowit-rust-v1"
    );
    assert_eq!(responses[3]["result"], Value::Null);
}
