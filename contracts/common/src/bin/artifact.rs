fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("Usage: allowit-contract-artifact <policy.rs> <intent.txt>")?;
    let intent_path = args.next().ok_or("Supply the owner intent text file")?;
    let binary = match args.next().as_deref() {
        None => false,
        Some("--binary") => true,
        _ => return Err("Expected optional --binary".into()),
    };
    if args.next().is_some() {
        return Err("Too many arguments".into());
    }
    let source = std::fs::read_to_string(path)?;
    let intent = std::fs::read_to_string(intent_path)?;
    let artifact = allowit_contract_core::compile_artifact(&source, &intent)?;
    let bytes = if binary {
        allowit_contract_core::binary::encode(&artifact)
            .map_err(|_| "Policy exceeds the binary chain profile")?
    } else {
        serde_json::to_vec(&artifact)?
    };
    if bytes.len() > allowit_contract_core::MAX_ARTIFACT_BYTES {
        return Err("The compiled artifact exceeds the contract size limit".into());
    }
    // No newline: hashing stdout gives the exact artifact_hash to activate.
    use std::io::Write;
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
