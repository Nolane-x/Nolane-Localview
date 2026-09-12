use std::collections::BTreeSet;

use localview_validation_lab::{LabError, LabSeed, LabSeedCatalog, LabSeedIdentity};
use serde_json::json;

fn seed(oracle_revision: &str, expected: &str) -> LabSeed {
    LabSeed {
        identity: LabSeedIdentity {
            seed_id: "LV-S041".into(),
            prediction_revision: "pred-r2".into(),
            oracle_revision: oracle_revision.into(),
        },
        family: "freshness".into(),
        spec_surface_refs: BTreeSet::from([841, 843, 1055]),
        input_fixture: json!({"generation": 7, "incarnation": "i1"}),
        expected_semantic_outcome: expected.into(),
        forbidden_outcomes: BTreeSet::from(["CURRENT".into()]),
        comparison_mode: "exact".into(),
        risk_if_missed: "high".into(),
    }
}

#[test]
fn duplicate_exact_seed_identity_is_rejected_instead_of_overwritten() {
    let first = seed("oracle-r1", "STALE");
    let mut conflicting = seed("oracle-r1", "CURRENT");
    conflicting.forbidden_outcomes = BTreeSet::from(["STALE".into()]);

    assert!(matches!(
        LabSeedCatalog::new("corpus-r1", vec![first, conflicting]),
        Err(LabError::DuplicateSeedIdentity { .. })
    ));
}

#[test]
fn oracle_correction_uses_a_new_revision_and_preserves_both_seeds() {
    let original = seed("oracle-r1", "STALE");
    let corrected = seed("oracle-r2", "RECONCILIATION_REQUIRED");
    let original_id = original.identity.clone();
    let corrected_id = corrected.identity.clone();

    let catalog = LabSeedCatalog::new("corpus-r2", vec![original, corrected]).unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(
        catalog.get(&original_id).unwrap().expected_semantic_outcome,
        "STALE"
    );
    assert_eq!(
        catalog
            .get(&corrected_id)
            .unwrap()
            .expected_semantic_outcome,
        "RECONCILIATION_REQUIRED"
    );
}

#[test]
fn catalog_digest_binds_oracle_revision_and_seed_content() {
    let baseline = LabSeedCatalog::new("corpus-r2", vec![seed("oracle-r1", "STALE")]).unwrap();
    let corrected = LabSeedCatalog::new(
        "corpus-r2",
        vec![seed("oracle-r2", "RECONCILIATION_REQUIRED")],
    )
    .unwrap();
    assert_ne!(
        baseline.canonical_digest().unwrap(),
        corrected.canonical_digest().unwrap()
    );

    let mut changed_content = seed("oracle-r1", "STALE");
    changed_content.input_fixture = json!({"generation": 8, "incarnation": "i1"});
    let changed = LabSeedCatalog::new("corpus-r2", vec![changed_content]).unwrap();
    assert_ne!(
        baseline.canonical_digest().unwrap(),
        changed.canonical_digest().unwrap()
    );
}

#[test]
fn catalog_digest_is_independent_of_set_and_seed_insertion_order() {
    let mut first = seed("oracle-r1", "STALE");
    first.spec_surface_refs = [1055, 841, 843].into_iter().collect();
    first.forbidden_outcomes = ["CURRENT".into(), "UNKNOWN".into()].into_iter().collect();

    let mut same = seed("oracle-r1", "STALE");
    same.spec_surface_refs = [843, 1055, 841].into_iter().collect();
    same.forbidden_outcomes = ["UNKNOWN".into(), "CURRENT".into()].into_iter().collect();

    let other = seed("oracle-r2", "RECONCILIATION_REQUIRED");
    let a = LabSeedCatalog::new("corpus-r2", vec![first, other.clone()]).unwrap();
    let b = LabSeedCatalog::new("corpus-r2", vec![other, same]).unwrap();

    assert_eq!(a.canonical_digest().unwrap(), b.canonical_digest().unwrap());
}

#[test]
fn seed_and_catalog_authority_fields_cannot_be_empty() {
    let mut invalid_seed = seed("oracle-r1", "STALE");
    invalid_seed.identity.seed_id.clear();
    assert!(matches!(
        LabSeedCatalog::new("corpus-r1", vec![invalid_seed]),
        Err(LabError::EmptyAuthorityField { .. })
    ));

    let mut invalid_prediction = seed("oracle-r1", "STALE");
    invalid_prediction.identity.prediction_revision.clear();
    assert!(matches!(
        LabSeedCatalog::new("corpus-r1", vec![invalid_prediction]),
        Err(LabError::EmptyAuthorityField { .. })
    ));

    let mut invalid_oracle = seed("oracle-r1", "STALE");
    invalid_oracle.identity.oracle_revision.clear();
    assert!(matches!(
        LabSeedCatalog::new("corpus-r1", vec![invalid_oracle]),
        Err(LabError::EmptyAuthorityField { .. })
    ));

    assert!(matches!(
        LabSeedCatalog::new("", vec![seed("oracle-r1", "STALE")]),
        Err(LabError::EmptyAuthorityField { .. })
    ));
}

#[test]
fn spec_surface_refs_use_the_normative_u32_schema() {
    fn require_u32_refs(_: &BTreeSet<u32>) {}

    let seed = seed("oracle-r1", "STALE");
    require_u32_refs(&seed.spec_surface_refs);
}

#[test]
fn catalog_digest_binds_corpus_revision() {
    let first = LabSeedCatalog::new("corpus-r1", vec![seed("oracle-r1", "STALE")]).unwrap();
    let second = LabSeedCatalog::new("corpus-r2", vec![seed("oracle-r1", "STALE")]).unwrap();

    assert_ne!(
        first.canonical_digest().unwrap(),
        second.canonical_digest().unwrap()
    );
}
