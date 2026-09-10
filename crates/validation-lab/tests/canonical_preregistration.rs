use std::collections::BTreeSet;

use localview_validation_lab::{
    CampaignLayer, CanonicalDigest, LabError, LabMetricKind, LabPreregistration,
    LabRevisionContext, LabSeedIdentity, PersistedPreregistrationReceipt, canonical_digest,
    canonical_json_bytes, validate_persisted_receipt,
};
use serde_json::json;

fn revision_context() -> LabRevisionContext {
    LabRevisionContext {
        lab_revision: "lab-r1".into(),
        seed_corpus_revision: "corpus-r2".into(),
        spec_revision_digest: "spec-digest-r1".into(),
        reference_reducer_revision: "reducer-r3".into(),
        mutation_catalog_revision: "mutation-r4".into(),
        comparison_profile_revision: "comparison-r5".into(),
        random_source_profile: "deterministic-seed-17".into(),
        platform_profile: None,
        start_sequence: 41,
    }
}

fn preregistration() -> LabPreregistration {
    LabPreregistration {
        revision_context: revision_context(),
        seed_catalog_digest: CanonicalDigest(
            "1111111111111111111111111111111111111111111111111111111111111111".into(),
        ),
        seed_identities: vec![LabSeedIdentity {
            seed_id: "LV-S041".into(),
            prediction_revision: "pred-r2".into(),
            oracle_revision: "oracle-r2".into(),
        }],
        campaign_layer: CampaignLayer::L1,
        expected_distinctions: BTreeSet::from(["fresh-vs-stale".into(), "principal-binding".into()]),
        model_bound: Some(10_000),
        assumptions: BTreeSet::from(["deterministic-scheduler".into(), "model-free".into()]),
        declared_metrics: BTreeSet::from([
            LabMetricKind::Suar,
            LabMetricKind::Wpdr,
            LabMetricKind::Pilr,
            LabMetricKind::Pdmr,
            LabMetricKind::Uobrr,
        ]),
        creation_sequence: 40,
    }
}

#[test]
fn canonical_json_sorts_maps_and_pins_numeric_encoding() {
    let a = json!({"z": {"b": 2, "a": 1}, "a": [3, 2, 1]});
    let b = json!({"a": [3, 2, 1], "z": {"a": 1, "b": 2}});
    assert_eq!(canonical_json_bytes(&a).unwrap(), canonical_json_bytes(&b).unwrap());
    assert_eq!(canonical_digest(&a).unwrap(), canonical_digest(&b).unwrap());

    let numeric = json!({"a": 1, "b": 1.5, "c": 1e6});
    assert_eq!(
        canonical_json_bytes(&numeric).unwrap(),
        br#"{"a":1,"b":1.5,"c":1000000.0}"#.to_vec()
    );
}

#[test]
fn preregistration_digest_is_semantic_and_set_order_independent() {
    let first = preregistration();
    let mut same = preregistration();
    same.expected_distinctions = ["principal-binding".into(), "fresh-vs-stale".into()]
        .into_iter()
        .collect();
    same.assumptions = ["model-free".into(), "deterministic-scheduler".into()]
        .into_iter()
        .collect();

    assert_eq!(first.prepare().unwrap().digest, same.prepare().unwrap().digest);

    let baseline = first.prepare().unwrap().digest;
    let mut changed = preregistration();
    changed.expected_distinctions.insert("unknown-outcome".into());
    assert_ne!(baseline, changed.prepare().unwrap().digest);

    let mut changed = preregistration();
    changed.revision_context.seed_corpus_revision = "corpus-r3".into();
    assert_ne!(baseline, changed.prepare().unwrap().digest);

    let mut changed = preregistration();
    changed.model_bound = Some(10_001);
    assert_ne!(baseline, changed.prepare().unwrap().digest);

    let mut changed = preregistration();
    changed.revision_context.comparison_profile_revision = "comparison-r6".into();
    assert_ne!(baseline, changed.prepare().unwrap().digest);

    let mut changed = preregistration();
    changed.revision_context.random_source_profile = "deterministic-seed-18".into();
    assert_ne!(baseline, changed.prepare().unwrap().digest);
}

#[test]
fn only_exact_external_persistence_acknowledgement_mints_validated_receipt() {
    let prepared = preregistration().prepare().unwrap();
    let receipt = PersistedPreregistrationReceipt {
        digest: prepared.digest.clone(),
        logical_sequence: 42,
        persistence_ref: "artifact://prereg/42".into(),
    };

    let validated = validate_persisted_receipt(&prepared, receipt.clone()).unwrap();
    assert_eq!(validated.digest(), &prepared.digest);
    assert_eq!(validated.logical_sequence(), 42);
    assert_eq!(validated.persistence_ref(), "artifact://prereg/42");

    let mut mismatched = receipt.clone();
    mismatched.digest = CanonicalDigest(
        "2222222222222222222222222222222222222222222222222222222222222222".into(),
    );
    assert!(matches!(
        validate_persisted_receipt(&prepared, mismatched),
        Err(LabError::PreregistrationPersistenceMismatch { .. })
    ));

    let mut zero_sequence = receipt.clone();
    zero_sequence.logical_sequence = 0;
    assert!(matches!(
        validate_persisted_receipt(&prepared, zero_sequence),
        Err(LabError::InvalidPersistenceReceipt { .. })
    ));

    let mut empty_ref = receipt;
    empty_ref.persistence_ref.clear();
    assert!(matches!(
        validate_persisted_receipt(&prepared, empty_ref),
        Err(LabError::InvalidPersistenceReceipt { .. })
    ));
}
