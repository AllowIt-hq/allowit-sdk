#[path = "../../test_support.rs"]
#[allow(dead_code)]
mod support;

use allowit_contract_core::{
    Artifact, Error, binary, prepare_binary_execution, prepare_execution, validate_binary_artifact,
};

fn convert(state: &mut allowit_contract_core::State) {
    let artifact: Artifact = serde_json::from_slice(&state.artifact).unwrap();
    state.artifact = binary::encode(&artifact).unwrap();
    state.mandate.artifact_hash = allowit_sdk::digest(&state.artifact);
}

#[test]
fn binary_round_trip_preserves_every_ir_variant_and_canonical_hash() {
    for source in [
        support::SIMPLE,
        support::INPUT,
        support::CONFIDENCE,
        support::SEMANTIC,
        "pub async fn evaluate(ctx: &Context) -> PolicyResult { let a: u64 = 2; let b: bool = !(a == 3); if b && ctx.amount_units > a { return fail(\"No\"); } else { allow_actions(ctx, &[\"research\"])?; } Ok(()) }",
    ] {
        let state = support::fixture(source);
        let artifact: Artifact = serde_json::from_slice(&state.artifact).unwrap();
        let bytes = binary::encode(&artifact).unwrap();
        let decoded = binary::decode(&bytes).unwrap();
        assert_eq!(decoded.ir, artifact.ir);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), state.artifact);
        assert_eq!(binary::encode(&decoded).unwrap(), bytes);
        assert_eq!(
            allowit_sdk::canonical_ir_hash(&decoded.ir).unwrap(),
            state.mandate.ir_hash
        );
    }
}

#[test]
fn binary_and_json_execute_the_same_program_and_reject_input() {
    for source in [
        support::SIMPLE,
        support::INPUT,
        support::CONFIDENCE,
        support::SEMANTIC,
    ] {
        let mut json = support::fixture(source);
        json.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
            key: [6; 32],
            key_id: "attester".into(),
            version: "1".into(),
        });
        let mut bin = json.clone();
        convert(&mut bin);
        for amount in [1, 1_000_000, 10_000_001, 100_000_001] {
            for lower in [8000, 9500] {
                let mut a = support::request(&json, amount);
                let mut b = support::request(&bin, amount);
                if source == support::SEMANTIC {
                    a.runtime_context = "{\"risk\":2}".into();
                    b.runtime_context = a.runtime_context.clone();
                    support::semantic_evidence(&json, &mut a, lower);
                    support::semantic_evidence(&bin, &mut b, lower);
                } else if source == support::CONFIDENCE {
                    support::evidence(&json, &mut a, lower, 9800);
                    support::evidence(&bin, &mut b, lower, 9800);
                }
                assert_eq!(
                    prepare_execution(&json, &a, 1000),
                    prepare_binary_execution(&bin, &b, 1000)
                );
            }
        }
    }
    let mut max = support::maximum_semantic_fixture();
    convert(&mut max);
    validate_binary_artifact(&max.mandate, &max.artifact).unwrap();
}

#[test]
fn binary_rejects_malformed_lengths_depth_nodes_tags_utf8_and_trailing_bytes() {
    let mut state = support::fixture(support::SIMPLE);
    convert(&mut state);
    for n in 0..state.artifact.len() {
        assert!(
            binary::decode(&state.artifact[..n]).is_err(),
            "accepted truncated prefix {n}"
        );
    }
    let mut trailing = state.artifact.clone();
    trailing.push(0);
    assert!(binary::decode(&trailing).is_err());
    let mut length = state.artifact.clone();
    length[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(binary::decode(&length).is_err());
    let mut utf8 = state.artifact.clone();
    utf8[10] = 255;
    assert!(binary::decode(&utf8).is_err());
    let mut magic = state.artifact.clone();
    magic[7] = b'2';
    assert!(binary::decode(&magic).is_err());
    // Skip the seven length-prefixed metadata strings; replace the IR stream.
    let mut offset = 8;
    for _ in 0..7 {
        let n = u16::from_le_bytes(state.artifact[offset..offset + 2].try_into().unwrap()) as usize;
        offset += 2 + n;
    }
    let header = &state.artifact[..offset];
    let mut deep = header.to_vec();
    deep.extend_from_slice(&1u16.to_le_bytes());
    deep.push(1); // expression statement
    deep.extend_from_slice(&[8; 64]); // deeply nested Not, must fail before leaf/allocation
    assert!(binary::decode(&deep).is_err());
    let mut nodes = header.to_vec();
    nodes.extend_from_slice(&257u16.to_le_bytes());
    nodes.extend_from_slice(&[0; 257]);
    assert!(binary::decode(&nodes).is_err());
    let mut tag = header.to_vec();
    tag.extend_from_slice(&1u16.to_le_bytes());
    tag.push(255);
    assert!(binary::decode(&tag).is_err());
    let mut boolean = header.to_vec();
    boolean.extend_from_slice(&1u16.to_le_bytes());
    boolean.extend_from_slice(&[1, 2, 2]);
    assert!(binary::decode(&boolean).is_err());
}

#[test]
fn binary_requires_exact_artifact_and_canonical_ir_hash_and_validates_signed_ir() {
    let mut state = support::fixture(support::SIMPLE);
    convert(&mut state);
    let mut altered = state.artifact.clone();
    altered[10] ^= 1;
    assert_eq!(
        validate_binary_artifact(&state.mandate, &altered).unwrap_err(),
        Error::ArtifactMismatch
    );
    let mut artifact = binary::decode(&state.artifact).unwrap();
    artifact.ir.statements.remove(1);
    let bytes = binary::encode(&artifact).unwrap();
    state.mandate.artifact_hash = allowit_sdk::digest(&bytes);
    assert_eq!(
        validate_binary_artifact(&state.mandate, &bytes).unwrap_err(),
        Error::ArtifactMismatch
    );
    // A forged version cannot bypass the shared validator even with a matching
    // transport digest. Encode directly to exercise the untrusted decoder path.
    let mut forged = state.artifact.clone();
    let mut offset = 8;
    for _ in 0..6 {
        let n = u16::from_le_bytes(forged[offset..offset + 2].try_into().unwrap()) as usize;
        offset += 2 + n;
    }
    forged[offset + 2] = b'x';
    state.mandate.artifact_hash = allowit_sdk::digest(&forged);
    assert_eq!(
        validate_binary_artifact(&state.mandate, &forged).unwrap_err(),
        Error::InvalidArtifact
    );
}

#[cfg(feature = "integer-json")]
#[test]
fn stellar_json_numbers_use_exact_integers_without_floating_point() {
    let mut state = support::fixture(support::SEMANTIC);
    convert(&mut state);
    state.mandate.evidence_authority = Some(allowit_contract_core::EvidenceAuthority {
        key: [6; 32],
        key_id: "attester".into(),
        version: "1".into(),
    });
    for json in [
        r#"{"risk":2.0}"#,
        r#"{"risk":2e0}"#,
        r#"{"risk":18446744073709551616}"#,
        r#"{"nested":[0.5],"risk":2}"#,
        r#"{"nested":-9223372036854775809,"risk":2}"#,
    ] {
        let mut request = support::request(&state, 1_000_000);
        request.runtime_context = json.into();
        support::semantic_evidence(&state, &mut request, 9500);
        assert_eq!(
            prepare_binary_execution(&state, &request, 1000),
            Err(Error::InvalidEvidence)
        );
    }
    let mut request = support::request(&state, 1_000_000);
    request.runtime_context =
        r#"{"high":18446744073709551615,"low":-9223372036854775808,"risk":2}"#.into();
    support::semantic_evidence(&state, &mut request, 9500);
    assert!(prepare_binary_execution(&state, &request, 1000).is_ok());
}
