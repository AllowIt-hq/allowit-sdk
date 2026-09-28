use crate::{compile, registry};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
};

#[derive(Default)]
pub struct LanguageServer {
    documents: BTreeMap<String, String>,
    shutdown: bool,
}
impl LanguageServer {
    /// Handle one JSON-RPC message. Notifications may return diagnostics notifications.
    pub fn handle(&mut self, message: Value) -> Vec<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let p = &message["params"];
        let response = |result: Value| json!({"jsonrpc":"2.0","id":id,"result":result});
        match method{
            "initialize"=>vec![response(json!({"capabilities":{"textDocumentSync":1,"hoverProvider":true,"completionProvider":{"triggerCharacters":["_"]},"experimental":{"allowitWorkflow":true}},"serverInfo":{"name":"AllowIt","version":env!("CARGO_PKG_VERSION")}}))],
            "shutdown"=>{self.shutdown=true;vec![response(Value::Null)]},
            "exit"=>vec![],
            "initialized"=>vec![],
            _ if self.shutdown=>if id.is_some(){vec![json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"The server has shut down."}})]}else{vec![]},
            "textDocument/didOpen"=>{let uri=p["textDocument"]["uri"].as_str().unwrap_or("");let source=p["textDocument"]["text"].as_str().unwrap_or("");self.documents.insert(uri.into(),source.into());vec![diagnostics(uri,source)]},
            "textDocument/didChange"=>{let uri=p["textDocument"]["uri"].as_str().unwrap_or("");if let Some(source)=p["contentChanges"].as_array().and_then(|a|a.last()).and_then(|v|v["text"].as_str()){self.documents.insert(uri.into(),source.into());vec![diagnostics(uri,source)]}else{vec![]}},
            "textDocument/didClose"=>{let uri=p["textDocument"]["uri"].as_str().unwrap_or("");self.documents.remove(uri);vec![json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[]}})]},
            "textDocument/completion"=>vec![response(json!(registry().iter().map(|f|json!({"label":f.name,"kind":3,"detail":f.signature,"documentation":f.description})).collect::<Vec<_>>()))],
            "textDocument/hover"=>{
                let uri=p["textDocument"]["uri"].as_str().unwrap_or("");let source=self.documents.get(uri).map(String::as_str).unwrap_or("");let line=p["position"]["line"].as_u64().unwrap_or(0) as usize;let column=p["position"]["character"].as_u64().unwrap_or(0) as usize;
                let offset=utf16_position(source,line,column);
                let help=compile(source).ok().and_then(|policy|policy.calls.into_iter().find(|c|c.start<=offset&&offset<c.end)).and_then(|call|registry().into_iter().find(|f|f.name==call.name));
                vec![response(help.map(|f|json!({"contents":{"kind":"markdown","value":format!("**{}**\n\n{}\n\n```rust\n{}\n```",f.title,f.description,f.signature)}})).unwrap_or(Value::Null))]
            },
            "allowit/workflow"=>{let uri=p["textDocument"]["uri"].as_str().unwrap_or("");let source=p["source"].as_str().or_else(||self.documents.get(uri).map(String::as_str)).unwrap_or("");vec![response(crate::process_value(json!({"operation":"workflow","source":source})))]},
            _=>if id.is_some(){vec![json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}})]}else{vec![]},
        }
    }
}
fn utf16_position(source: &str, line: usize, column: usize) -> usize {
    source
        .split_inclusive('\n')
        .take(line)
        .map(|s| s.encode_utf16().count())
        .sum::<usize>()
        + column
}
fn diagnostics(uri: &str, source: &str) -> Value {
    let errors = match compile(source) {
        Ok(_) => vec![],
        Err(e) => {
            let line = e.line.unwrap_or(1).saturating_sub(1);
            let char_col = e.column.unwrap_or(1).saturating_sub(1);
            let text = source.lines().nth(line).unwrap_or("");
            let column = text
                .chars()
                .take(char_col)
                .map(char::len_utf16)
                .sum::<usize>();
            vec![
                json!({"range":{"start":{"line":line,"character":column},"end":{"line":line,"character":column+1}},"severity":1,"source":"AllowIt","code":e.code,"message":e.message}),
            ]
        }
    };
    json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":errors}})
}
/// Run the standard LSP Content-Length framed transport over stdin/stdout.
pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut server = LanguageServer::default();
    loop {
        let mut length = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                return Ok(());
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("Content-Length")
            {
                length = value.trim().parse::<usize>().ok();
            }
        }
        let length = length
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing Content-Length"))?;
        if length > 262144 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "LSP request exceeds 256 KiB",
            ));
        }
        let mut body = vec![0; length];
        std::io::Read::read_exact(&mut reader, &mut body)?;
        let message: Value = serde_json::from_slice(&body)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if message["method"] == "exit" {
            return Ok(());
        }
        for response in server.handle(message) {
            let bytes = serde_json::to_vec(&response)?;
            write!(writer, "Content-Length: {}\r\n\r\n", bytes.len())?;
            writer.write_all(&bytes)?;
            writer.flush()?;
        }
    }
}
