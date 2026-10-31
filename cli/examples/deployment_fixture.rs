//! Public cross-language handoff fixture. All keys are deterministic test public keys.
use allowit_cli::{
    handoff,
    native::{
        client::{Config, Deployment},
        crypto::Key,
        policy::Policy,
    },
};
fn main() {
    let policy = Policy::generate("solana:devnet", "Spend up to 5 test tokens per day").unwrap();
    let deployment = Deployment {
        network: policy.network.clone(),
        source_bundle: policy.source_bundle.clone(),
        policy: Key([7; 32]),
        policy_data: Key([8; 32]),
        custody: Key([9; 32]),
    };
    let config = Config {
        network: policy.network.clone(),
        mint: Some(Key([4; 32])),
        executor: Some(Key([2; 32])),
        authority: Some(Key([3; 32])),
        deployment: Some(deployment),
    };
    println!(
        "{}",
        handoff::signing_url(
            "https://app.allowit.xyz",
            "cli-deploy-0001",
            Key([1; 32]),
            "1",
            &policy,
            &config
        )
        .unwrap()
    );
}
