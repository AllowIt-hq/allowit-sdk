use allowit_sdk::lsp::LanguageServer;
use serde_json::json;
#[test]
fn diagnostics_hover_completion_and_workflow_share_the_compiler() {
    let mut server = LanguageServer::default();
    let init = server.handle(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}));
    assert_eq!(init[0]["result"]["capabilities"]["hoverProvider"], true);
    let source = "pub async fn evaluate(ctx: &Context) -> PolicyResult { allowit::day_spent_cap(grant::day_spent_cap)?; Ok(()) }";
    let open=server.handle(json!({"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///policy.rs","text":source}}}));
    assert_eq!(open[0]["params"]["diagnostics"], json!([]));
    let hover=server.handle(json!({"id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///policy.rs"},"position":{"line":0,"character":source.find("day_spent_cap").unwrap()}}}));
    assert!(
        hover[0]["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Daily spent limit")
    );
    let completion = server.handle(json!({"id":3,"method":"textDocument/completion","params":{}}));
    let items = completion[0]["result"].as_array().unwrap();
    let labels: std::collections::BTreeSet<_> = items
        .iter()
        .map(|item| item["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels.len(), items.len());
    assert!(labels.contains("allowit::recipients"));
    assert!(labels.contains("allowit::check_preference"));
    let workflow=server.handle(json!({"id":4,"method":"allowit/workflow","params":{"textDocument":{"uri":"file:///policy.rs"}}}));
    assert_eq!(
        workflow[0]["result"]["policy"]["native_prefix"],
        json!(["day_spent_cap"])
    );
    assert!(
        workflow[0]["result"]["policy"]["workflow"][0]["source"]
            .as_str()
            .unwrap()
            .contains("allowit::day_spent_cap(grant::day_spent_cap)")
    );
    let changed=server.handle(json!({"method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///policy.rs"},"contentChanges":[{"text":"bad source"}]}}));
    assert_eq!(
        changed[0]["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn diagnostic_columns_use_utf16_after_non_bmp_text() {
    let source = "pub async fn evaluate(ctx: &Context) -> PolicyResult { let message = \"😀\"; ctx.clone(); Ok(()) }";
    let mut server = LanguageServer::default();
    let result=server.handle(json!({"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///unicode.rs","text":source}}}));
    assert_eq!(
        result[0]["params"]["diagnostics"][0]["range"]["start"]["character"],
        source[..source.find("ctx.clone").unwrap()]
            .encode_utf16()
            .count()
    );
}
