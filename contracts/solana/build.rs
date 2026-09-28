fn main() {
    println!("cargo:rerun-if-env-changed=ALLOWIT_SOLANA_NETWORK");
    // Solana exposes no trustworthy genesis/cluster sysvar to programs. Deploy
    // a separately bound artifact on each cluster and verify the deployment's
    // genesis out of band. Refuse to produce an unbound deployable binary.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("solana") {
        match std::env::var("ALLOWIT_SOLANA_NETWORK").as_deref() {
            Ok("mainnet" | "testnet" | "devnet") => {}
            _ => panic!("Set ALLOWIT_SOLANA_NETWORK explicitly to mainnet, testnet, or devnet"),
        }
    }
}
