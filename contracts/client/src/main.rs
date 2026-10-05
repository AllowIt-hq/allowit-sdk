use std::io::{self, BufRead, Read, Write};

fn answer(input: &str) -> String {
    let result = serde_json::from_str(input)
        .map_err(|e| e.to_string())
        .and_then(|value| allowit_contract_client::dispatch(&value));
    serde_json::to_string(&match result {
        Ok(value) => value,
        Err(message) => serde_json::json!({"ok":false,"error":message}),
    })
    .expect("JSON response is serializable")
}

fn main() -> io::Result<()> {
    let mut output = io::BufWriter::new(io::stdout().lock());
    if std::env::args().any(|arg| arg == "--json-lines") {
        for line in io::stdin().lock().lines() {
            writeln!(output, "{}", answer(&line?))?;
            output.flush()?;
        }
    } else {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        writeln!(output, "{}", answer(&input))?;
    }
    output.flush()
}
