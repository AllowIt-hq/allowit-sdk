use serde_json::json;
use std::{
    env, fs,
    io::{self, Read},
    process::ExitCode,
};
fn read(path: &str) -> Result<String, String> {
    if path == "-" {
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| e.to_string())?;
        Ok(s)
    } else {
        fs::read_to_string(path).map_err(|e| e.to_string())
    }
}
fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("help");
    if command == "lsp" {
        return allowit_sdk::lsp::run_stdio().map_err(|e| e.to_string());
    }
    if command == "help" || command == "--help" {
        println!(
            "allowit compile|check|workflow POLICY.rs\nallowit evaluate POLICY.rs CONTEXT.json [oracle|contract]\nallowit registry\nallowit json [REQUEST.json|-]\nallowit lsp\n\nUse - to read source or a JSON request from stdin."
        );
        return Ok(());
    }
    let request = match command {
        "registry" => json!({"operation":"registry"}),
        "json" => {
            let input = read(args.get(1).map(String::as_str).unwrap_or("-"))?;
            serde_json::from_str(&input).map_err(|e| e.to_string())?
        }
        "compile" | "check" | "workflow" => {
            json!({"operation":command,"source":read(args.get(1).ok_or("Supply a policy source file.")?)?})
        }
        "evaluate" => {
            json!({"operation":"evaluate","source":read(args.get(1).ok_or("Supply a policy source file.")?)?,"context":serde_json::from_str::<serde_json::Value>(&read(args.get(2).ok_or("Supply a context JSON file.")?)?).map_err(|e|e.to_string())?,"profile":args.get(3).map(String::as_str).unwrap_or("oracle")})
        }
        _ => return Err("Unknown command. Run allowit --help.".into()),
    };
    let result = allowit_sdk::process_value(request);
    println!(
        "{}",
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
    );
    if result["ok"] == false {
        return Err("Policy operation failed.".into());
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
