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
        catalog.get(&corrected_id).unwrap().expected_semantic_outcome,
        "RECONCILIATION_REQUIRED"
    );
}

#[test]
fn seed_and_catalog_authority_fields_cannot_be_empty() {
    let mut invalid_seed = seed("oracle-r1", "STALE");
    invalid_seed.identity.seed_id.clear();
    assert!(matches!(
        LabSeedCatalog::new("corpus-r1", vec![invalid_seed]),
        Err(LabError::EmptyAuthorityField { .. })
    ));

    assert!(matches!(
        LabSeedCatalog::new("", vec![seed("oracle-r1", "STALE")]),
        Err(LabError::EmptyAuthorityField { .. })
    ));
}
