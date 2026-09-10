use localview_validation_lab::{
    CanonicalArtifact, LabArtifactKind, LabArtifactState, ResearchResultClass,
};
use serde::Serialize;
use serde_json::json;

#[test]
fn first_slice_artifact_helpers_are_canonical_and_repeatable() {
    let preregistration = json!({"z": 2, "a": 1});
    let first = CanonicalArtifact::preregistration(&preregistration).unwrap();
    let second = CanonicalArtifact::preregistration(&preregistration).unwrap();

    assert_eq!(first.kind, LabArtifactKind::Preregistration);
    assert_eq!(first, second);
    assert_eq!(first.canonical_bytes, br#"{"a":1,"z":2}"#.to_vec());

    let seed_catalog = json!({"revision": "seed-v1", "seeds": ["s1"]});
    let seed_first = CanonicalArtifact::seed_catalog(&seed_catalog).unwrap();
    let seed_second = CanonicalArtifact::seed_catalog(&seed_catalog).unwrap();
    assert_eq!(seed_first.kind, LabArtifactKind::SeedCatalog);
    assert_eq!(seed_first, seed_second);

    let results = json!({"result_class": "preregistered_seed_pass", "count": 1});
    let result_first = CanonicalArtifact::results(&results).unwrap();
    let result_second = CanonicalArtifact::results(&results).unwrap();
    assert_eq!(result_first.kind, LabArtifactKind::Results);
    assert_eq!(result_first, result_second);
}

#[test]
fn artifact_digest_changes_when_semantic_payload_changes() {
    let before = CanonicalArtifact::results(&json!({"count": 1})).unwrap();
    let after = CanonicalArtifact::results(&json!({"count": 2})).unwrap();

    assert_ne!(before.digest, after.digest);
    assert_ne!(before.canonical_bytes, after.canonical_bytes);
}

#[derive(Serialize)]
struct ManifestEntry<'a> {
    kind: LabArtifactKind,
    state: LabArtifactState,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<&'a [u8]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<&'a str>,
}

#[test]
fn not_run_report_has_no_fabricated_payload_or_digest() {
    let entry = ManifestEntry {
        kind: LabArtifactKind::MutationReport,
        state: LabArtifactState::NotRun,
        payload: None,
        digest: None,
    };
    let value = serde_json::to_value(entry).unwrap();

    assert_eq!(value["kind"], "mutation_report");
    assert_eq!(value["state"], "not_run");
    assert!(value.get("payload").is_none());
    assert!(value.get("digest").is_none());
}

#[test]
fn artifact_vocabulary_cannot_serialize_a_proved_result_class() {
    let variants = [
        ResearchResultClass::ExploratoryObservation,
        ResearchResultClass::PreregisteredSeedPass,
        ResearchResultClass::CounterexampleFound,
        ResearchResultClass::NoCounterexampleWithinBoundN,
        ResearchResultClass::MutantKilled,
        ResearchResultClass::MutantSurvived,
        ResearchResultClass::DifferentialEquivalentWithinVectorSet,
        ResearchResultClass::DifferentialDivergenceFound,
        ResearchResultClass::PropertyCampaignPassN,
        ResearchResultClass::RealProviderIntegrationPass,
        ResearchResultClass::IndependentReplicationPass,
    ];

    for class in variants {
        let serialized = serde_json::to_string(&class).unwrap();
        assert!(!serialized.to_ascii_lowercase().contains("proved"));
    }
}
