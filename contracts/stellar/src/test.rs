extern crate std;

#[path = "../../test_support.rs"]
mod support;

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::token::{Client as TokenClient, StellarAssetClient};

struct Fixture {
    env: Env,
    contract: Address,
    activation: Activation,
    policy: State,
    owner: Address,
    compiler: Address,
    executor: Address,
    recipient: Address,
    asset: Address,
}

impl Fixture {
    fn new(source: &str) -> Self {
        Self::with_policy(support::fixture(source))
    }
    fn with_policy(mut policy: State) -> Self {
        let artifact: allowit_contract_core::Artifact =
            serde_json::from_slice(&policy.artifact).unwrap();
        policy.artifact = allowit_contract_core::binary::encode(&artifact).unwrap();
        policy.mandate.artifact_hash = allowit_sdk::digest(&policy.artifact);
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|ledger| {
            ledger.timestamp = 1000;
            ledger.sequence_number = 1000;
            ledger.network_id = env
                .crypto()
                .sha256(&Bytes::from_slice(
                    &env,
                    b"Test SDF Network ; September 2015",
                ))
                .to_array();
        });
        let contract = env.register(AllowIt, ());
        let owner = Address::generate(&env);
        let compiler = Address::generate(&env);
        let executor = Address::generate(&env);
        let recipient = Address::generate(&env);
        // Register the real SAC for Circle's canonical Testnet USDC asset.
        // Its issuer account exists only in this isolated host ledger.
        use soroban_sdk::xdr;
        use std::rc::Rc;
        let encoded_asset = usdc_asset_xdr(&env).unwrap();
        let encoded = encoded_asset.to_alloc_vec();
        let issuer = xdr::AccountId(xdr::PublicKey::PublicKeyTypeEd25519(xdr::Uint256(
            encoded[12..44].try_into().unwrap(),
        )));
        env.host()
            .add_ledger_entry(
                &Rc::new(xdr::LedgerKey::Account(xdr::LedgerKeyAccount {
                    account_id: issuer.clone(),
                })),
                &Rc::new(xdr::LedgerEntry {
                    data: xdr::LedgerEntryData::Account(xdr::AccountEntry {
                        account_id: issuer,
                        balance: 1_000_000_000,
                        flags: 0,
                        home_domain: Default::default(),
                        inflation_dest: None,
                        num_sub_entries: 0,
                        seq_num: xdr::SequenceNumber(0),
                        thresholds: xdr::Thresholds([1; 4]),
                        signers: xdr::VecM::default(),
                        ext: xdr::AccountEntryExt::V0,
                    }),
                    last_modified_ledger_seq: 0,
                    ext: xdr::LedgerEntryExt::V0,
                }),
                None,
            )
            .unwrap();
        let asset = env.as_contract(&contract, || {
            env.deployer().with_stellar_asset(encoded_asset).deploy()
        });
        StellarAssetClient::new(&env, &asset).mint(&owner, &2_000_000_000);
        let m = &mut policy.mandate;
        m.owner = address_identity(&env, &owner);
        m.compiler = address_identity(&env, &compiler);
        m.executor = address_identity(&env, &executor);
        m.recipient = address_identity(&env, &recipient);
        m.asset = address_identity(&env, &asset);
        m.asset_decimals = 7;
        m.allocation_units *= 10;
        m.network = "stellar:testnet".into();
        m.recipient_address = address_text(&recipient).unwrap();
        let activation = Activation {
            owner: owner.clone(),
            compiler: compiler.clone(),
            executor: executor.clone(),
            evidence_authority: None,
            recipient: recipient.clone(),
            asset: asset.clone(),
            mandate: Bytes::from_slice(&env, &borsh::to_vec(m).unwrap()),
            artifact: Bytes::from_slice(&env, &policy.artifact),
        };
        TokenClient::new(&env, &asset).approve(&owner, &contract, &1_000_000_000, &5000);
        Self {
            env,
            contract,
            activation,
            policy,
            owner,
            compiler,
            executor,
            recipient,
            asset,
        }
    }
    fn client(&self) -> AllowItClient<'_> {
        AllowItClient::new(&self.env, &self.contract)
    }
    fn request(&self, state: &State, amount: u64) -> Bytes {
        Bytes::from_slice(
            &self.env,
            &borsh::to_vec(&support::request(state, amount)).unwrap(),
        )
    }
    fn state(&self, id: &BytesN<32>) -> State {
        borsh::from_slice(&self.client().state(id).to_alloc_vec()).unwrap()
    }
    fn balance(&self) -> i128 {
        TokenClient::new(&self.env, &self.asset).balance(&self.recipient)
    }
}

#[test]
fn real_token_transfer_binds_authorities_action_network_asset_recipient_and_replay() {
    let f = Fixture::new(support::SIMPLE);
    let id = f.client().activate(&f.activation);
    let auth = f.env.auths();
    assert!(auth.iter().any(|(address, _)| address == &f.owner));
    assert!(auth.iter().any(|(address, _)| address == &f.compiler));
    for field in 0..7 {
        let mut bad = support::request(&f.policy, 100_000_000);
        match field {
            0 => bad.asset[0] ^= 1,
            1 => bad.recipient[0] ^= 1,
            2 => bad.network = "stellar-mainnet".into(),
            3 => bad.action = "other".into(),
            4 => bad.revision += 1,
            5 => bad.source_hash = "0".repeat(64),
            _ => bad.ir_hash = "0".repeat(64),
        }
        assert_eq!(
            f.client().try_execute(
                &id,
                &Bytes::from_slice(&f.env, &borsh::to_vec(&bad).unwrap())
            ),
            Err(Ok(Error::BindingMismatch))
        );
        assert_eq!(f.balance(), 0);
    }
    let first = f.request(&f.policy, 100_000_000);
    assert_eq!(f.client().execute(&id, &first), 100_000_000);
    assert!(
        f.env
            .auths()
            .iter()
            .any(|(address, _)| address == &f.executor)
    );
    assert_eq!(f.balance(), 100_000_000);
    assert_eq!(f.client().try_execute(&id, &first), Err(Ok(Error::Replay)));
    for _ in 1..10 {
        let state = f.state(&id);
        f.client().execute(&id, &f.request(&state, 100_000_000));
    }
    assert_eq!(f.balance(), 1_000_000_000);
    let state = f.state(&id);
    assert_eq!(
        f.client().try_execute(&id, &f.request(&state, 10)),
        Err(Ok(Error::BudgetExceeded))
    );
    f.client().revoke(&id);
    assert_eq!(
        f.client().try_execute(&id, &f.request(&state, 10)),
        Err(Ok(Error::Inactive))
    );
}

#[test]
fn input_failure_rolls_back_and_cannot_be_bypassed_by_oracle_answers() {
    let f = Fixture::new(support::INPUT);
    let id = f.client().activate(&f.activation);
    let before = f.client().state(&id);
    assert_eq!(
        f.client()
            .try_execute(&id, &f.request(&f.policy, 10_000_000)),
        Err(Ok(Error::UserInputRequired))
    );
    assert_eq!(f.client().state(&id), before);
    assert_eq!(f.balance(), 0);
    // Execute's typed request has no answer channel; no receipt can mask input.
    assert_eq!(f.state(&id).next_nonce, 0);
}

#[test]
fn missing_auth_forged_compiler_binding_and_altered_artifact_cannot_activate() {
    let f = Fixture::new(support::SIMPLE);
    f.env.mock_auths(&[]);
    assert!(f.client().try_activate(&f.activation).is_err());
    f.env.mock_all_auths();
    let mut wrong = f.activation.clone();
    wrong.compiler = Address::generate(&f.env);
    assert_eq!(
        f.client().try_activate(&wrong),
        Err(Ok(Error::BindingMismatch))
    );
    let mut altered = f.activation.clone();
    altered.artifact.set(0, b' ');
    assert_eq!(
        f.client().try_activate(&altered),
        Err(Ok(Error::ArtifactMismatch))
    );
    let id = f.client().activate(&f.activation);
    assert_eq!(
        f.client().try_activate(&f.activation),
        Err(Ok(Error::AlreadyInitialized))
    );
    f.env.mock_auths(&[]);
    assert!(
        f.client()
            .try_execute(&id, &f.request(&f.policy, 100_000_000))
            .is_err()
    );
    assert!(f.client().try_revoke(&id).is_err());
    assert_eq!(f.balance(), 0);
}

#[test]
fn confidence_requires_explicit_attester_auth_and_fresh_exact_action() {
    let mut f = Fixture::new(support::CONFIDENCE);
    let attester = Address::generate(&f.env);
    f.policy.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
        key: address_identity(&f.env, &attester),
        key_id: "merchant-oracle-v1".into(),
        version: "1".into(),
    });
    f.activation.evidence_authority = Some(attester.clone());
    f.activation.mandate = Bytes::from_slice(&f.env, &borsh::to_vec(&f.policy.mandate).unwrap());
    let id = f.client().activate(&f.activation);
    let mut req = support::request(&f.policy, 10_000_000);
    assert_eq!(
        f.client().try_execute(
            &id,
            &Bytes::from_slice(&f.env, &borsh::to_vec(&req).unwrap())
        ),
        Err(Ok(Error::EvidenceRequired))
    );
    support::evidence(&f.policy, &mut req, 9200, 9800);
    let encoded = Bytes::from_slice(&f.env, &borsh::to_vec(&req).unwrap());
    f.env.mock_auths(&[MockAuth {
        address: &f.executor,
        invoke: &MockAuthInvoke {
            contract: &f.contract,
            fn_name: "execute",
            args: (id.clone(), encoded.clone()).into_val(&f.env),
            sub_invokes: &[],
        },
    }]);
    assert!(f.client().try_execute(&id, &encoded).is_err());
    f.env.mock_all_auths();
    let mut altered = req.clone();
    altered.amount_units += 10;
    assert_eq!(
        f.client().try_execute(
            &id,
            &Bytes::from_slice(&f.env, &borsh::to_vec(&altered).unwrap())
        ),
        Err(Ok(Error::InvalidEvidence))
    );
    let mut expired = req.clone();
    expired.evidence.as_mut().unwrap().expires_at = 999;
    assert_eq!(
        f.client().try_execute(
            &id,
            &Bytes::from_slice(&f.env, &borsh::to_vec(&expired).unwrap())
        ),
        Err(Ok(Error::InvalidEvidence))
    );
    let mut ambiguous = req.clone();
    support::evidence(&f.policy, &mut ambiguous, 8000, 9500);
    assert_eq!(
        f.client().try_execute(
            &id,
            &Bytes::from_slice(&f.env, &borsh::to_vec(&ambiguous).unwrap())
        ),
        Err(Ok(Error::UserInputRequired))
    );
    assert_eq!(f.balance(), 0);
    f.client().execute(&id, &encoded);
    assert!(
        f.env
            .auths()
            .iter()
            .any(|(address, _)| address == &attester)
    );
    assert_eq!(f.balance(), 10_000_000);
}

#[test]
fn failed_token_transfer_cannot_consume_policy_budget_or_nonce() {
    let f = Fixture::new(support::SIMPLE);
    let id = f.client().activate(&f.activation);
    let before = f.client().state(&id);
    TokenClient::new(&f.env, &f.asset).approve(&f.owner, &f.contract, &0, &5000);
    assert!(
        f.client()
            .try_execute(&id, &f.request(&f.policy, 100_000_000))
            .is_err()
    );
    assert_eq!(f.client().state(&id), before);
    assert_eq!(f.balance(), 0);
}

#[test]
fn semantic_snapshot_binds_runtime_facts_and_original_owner_intent() {
    let mut f = Fixture::new(support::SEMANTIC);
    let attester = Address::generate(&f.env);
    f.policy.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
        key: address_identity(&f.env, &attester),
        key_id: "semantics-v1".into(),
        version: "1".into(),
    });
    f.activation.evidence_authority = Some(attester);
    f.activation.mandate = Bytes::from_slice(&f.env, &borsh::to_vec(&f.policy.mandate).unwrap());
    let id = f.client().activate(&f.activation);
    let mut req = support::request(&f.policy, 10_000_000);
    req.runtime_context = "{\"risk\":3}".into();
    support::semantic_evidence(&f.policy, &mut req, 9200);
    let mut altered = req.clone();
    altered.runtime_context = "{\"risk\":0}".into();
    assert_eq!(
        f.client().try_execute(
            &id,
            &Bytes::from_slice(&f.env, &borsh::to_vec(&altered).unwrap())
        ),
        Err(Ok(Error::InvalidEvidence))
    );
    f.client().execute(
        &id,
        &Bytes::from_slice(&f.env, &borsh::to_vec(&req).unwrap()),
    );
    assert_eq!(f.balance(), 10_000_000);
}

#[test]
fn new_revision_supersedes_old_mandate_and_unknown_assets_are_rejected() {
    let f = Fixture::new(support::SIMPLE);
    let old_id = f.client().activate(&f.activation);
    let mut new_mandate = f.policy.mandate.clone();
    new_mandate.revision = 2;
    let mut new_activation = f.activation.clone();
    new_activation.mandate = Bytes::from_slice(&f.env, &borsh::to_vec(&new_mandate).unwrap());
    let new_id = f.client().activate(&new_activation);
    assert_ne!(old_id, new_id);
    let mut downgraded = new_mandate.clone();
    downgraded.revision = 1;
    downgraded.expires_at += 1; // New envelope bytes cannot revive revision one.
    let mut downgrade_activation = new_activation.clone();
    downgrade_activation.mandate = Bytes::from_slice(&f.env, &borsh::to_vec(&downgraded).unwrap());
    assert_eq!(
        f.client().try_activate(&downgrade_activation),
        Err(Ok(Error::BindingMismatch))
    );
    assert_eq!(
        f.client()
            .try_execute(&old_id, &f.request(&f.policy, 10_000_000)),
        Err(Ok(Error::Inactive))
    );
    assert_eq!(f.balance(), 0);
    let wrong_asset = f
        .env
        .register_stellar_asset_contract_v2(Address::generate(&f.env))
        .address();
    new_mandate.revision = 3;
    new_mandate.asset = address_identity(&f.env, &wrong_asset);
    new_activation.asset = wrong_asset;
    new_activation.mandate = Bytes::from_slice(&f.env, &borsh::to_vec(&new_mandate).unwrap());
    assert_eq!(
        f.client().try_activate(&new_activation),
        Err(Ok(Error::BindingMismatch))
    );
}

#[test]
fn compiled_wasm_runs_in_the_soroban_vm() {
    let Ok(path) = std::env::var("ALLOWIT_STELLAR_WASM") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must supply the compiled Wasm artifact"
        );
        return;
    };
    let wasm = std::fs::read(path).unwrap();
    for (label, policy, input_required, evidence) in [
        ("pass", support::fixture(support::SIMPLE), false, false),
        ("user-input", support::fixture(support::INPUT), true, false),
        (
            "maximum-semantic",
            support::maximum_semantic_fixture(),
            false,
            true,
        ),
    ] {
        let mut f = Fixture::with_policy(policy);
        if evidence {
            let authority = Address::generate(&f.env);
            f.policy.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
                key: address_identity(&f.env, &authority),
                key_id: "evidence-v1".into(),
                version: "1".into(),
            });
            f.activation.evidence_authority = Some(authority);
            f.activation.mandate =
                Bytes::from_slice(&f.env, &borsh::to_vec(&f.policy.mandate).unwrap());
        }
        // This is the compiled artifact, not the native Rust entry point.
        // Wallet/SAC setup and Wasm upload/deploy are distinct transactions.
        f.env.budget().reset_default();
        std::println!("Soroban Wasm {label} register: wasm={}B", wasm.len());
        let registration = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.env.deployer().upload_contract_wasm(wasm.as_slice())
        }));
        if let Err(error) = registration {
            std::println!(
                "Soroban Wasm {label} upload failed: CPU={} memory={}",
                f.env.budget().cpu_instruction_cost(),
                f.env.budget().memory_bytes_cost()
            );
            std::panic::resume_unwind(error);
        }
        let wasm_hash = registration.unwrap();
        std::println!(
            "Soroban Wasm {label} upload: CPU={} memory={}",
            f.env.budget().cpu_instruction_cost(),
            f.env.budget().memory_bytes_cost()
        );
        f.env.budget().reset_default();
        f.contract = f
            .env
            .deployer()
            .with_address(f.owner.clone(), BytesN::from_array(&f.env, &[73; 32]))
            .deploy_v2(wasm_hash, ());
        std::println!(
            "Soroban Wasm {label} deploy: CPU={} memory={}",
            f.env.budget().cpu_instruction_cost(),
            f.env.budget().memory_bytes_cost()
        );
        f.env.budget().reset_default();
        TokenClient::new(&f.env, &f.asset).approve(&f.owner, &f.contract, &1_000_000_000, &5000);
        f.env.budget().reset_default();
        let id = f.client().activate(&f.activation);
        std::println!(
            "Soroban Wasm {label} activate: CPU={} memory={} wasm={}B",
            f.env.budget().cpu_instruction_cost(),
            f.env.budget().memory_bytes_cost(),
            wasm.len()
        );
        let before = f.client().state(&id);
        let mut request = support::request(&f.policy, 100_000_000);
        if evidence {
            f.env.budget().reset_default();
            request.runtime_context = r#"{"risk":2}"#.into();
            assert_eq!(
                f.client().try_execute(
                    &id,
                    &Bytes::from_slice(&f.env, &borsh::to_vec(&request).unwrap())
                ),
                Err(Ok(Error::EvidenceRequired))
            );
            support::semantic_evidence(&f.policy, &mut request, 9500);
        }
        let request = Bytes::from_slice(&f.env, &borsh::to_vec(&request).unwrap());
        f.env.budget().reset_default();
        if input_required {
            assert_eq!(
                f.client().try_execute(&id, &request),
                Err(Ok(Error::UserInputRequired))
            );
        } else {
            f.client().execute(&id, &request);
        }
        std::println!(
            "Soroban Wasm {label} execute: CPU={} memory={}",
            f.env.budget().cpu_instruction_cost(),
            f.env.budget().memory_bytes_cost()
        );
        if input_required {
            assert_eq!(f.client().state(&id), before);
            assert_eq!(f.balance(), 0);
        } else {
            assert_eq!(f.balance(), 100_000_000);
        }
    }
}

#[test]
fn compact_sha256_matches_standard_vectors() {
    assert_eq!(
        allowit_sdk::digest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        allowit_sdk::digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        allowit_sdk::digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn stellar_rejects_artifacts_above_its_canonical_size_profile() {
    let mut f = Fixture::with_policy(support::maximum_semantic_fixture());
    let artifact = allowit_contract_core::binary::decode(&f.policy.artifact).unwrap();
    assert_eq!(serde_json::to_vec(&artifact).unwrap().len(), 4096);
    assert!(artifact.original_intent.len() < 2048);
    // Add one intent byte without changing the Program, preserving valid binary
    // framing and IR hash. The owner/compiler authorize the new artifact digest.
    let mut bytes = f.policy.artifact.clone();
    let len = u16::from_le_bytes(bytes[8..10].try_into().unwrap());
    bytes[8..10].copy_from_slice(&(len + 1).to_le_bytes());
    bytes.insert(10 + usize::from(len), b'x');
    f.policy.mandate.artifact_hash = allowit_sdk::digest(&bytes);
    f.activation.mandate = Bytes::from_slice(&f.env, &borsh::to_vec(&f.policy.mandate).unwrap());
    f.activation.artifact = Bytes::from_slice(&f.env, &bytes);
    assert_eq!(
        f.client().try_activate(&f.activation),
        Err(Ok(Error::InvalidArtifact))
    );
}
