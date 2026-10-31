//! Shared helpers for the policy compiler commands (`compile`, `check`,
//! `workflow`, `evaluate`, `stellar-native`, `registry`, `json`, `lsp`). These
//! commands run the SDK locally and send nothing.
use crate::error::{Error, Result};
use serde_json::Value;
use std::io::Read;

/// A compiler command failure: exit code 1 with the message on stderr.
pub(super) fn failure(message: impl Into<String>) -> Error {
    Error {
        code: 1,
        message: message.into(),
        status: None,
        config: false,
    }
}

/// Reads a policy or JSON file. `-` reads stdin.
pub(super) fn read(path: &str) -> Result<String> {
    if path == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| failure(e.to_string()))?;
        Ok(s)
    } else {
        std::fs::read_to_string(path).map_err(|e| failure(e.to_string()))
    }
}

/// Reads the required policy source argument.
pub(super) fn source(args: &[String]) -> Result<String> {
    read(
        args.first()
            .ok_or_else(|| failure("Supply a policy source file."))?,
    )
}

/// Runs one wire-protocol request and prints the pretty JSON result.
/// Exits 1 after printing when the result has `"ok": false`.
pub(super) fn process(request: Value, stdout: &mut String) -> Result<i32> {
    let mut result = allowit_sdk::process_value(request);
    // MPP enables serde_json/preserve_order in this binary. Print keys in the
    // sorted order of the standalone compiler output.
    result.sort_all_objects();
    *stdout += &serde_json::to_string_pretty(&result).map_err(|e| failure(e.to_string()))?;
    stdout.push('\n');
    if result["ok"] == false {
        return Err(failure("Policy operation failed."));
    }
    Ok(0)
}
