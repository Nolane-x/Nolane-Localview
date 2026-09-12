use std::collections::BTreeSet;

use localview_validation_lab::{
    DifferentialVector, DifferentialVectorSetInput, LabError, LabFailureFlag, LabMetricKind,
    MetricStatus, ResultEvidence, adapt_differential_vector_set, reduce_metric_observations,
};

fn vector(id: &str, reference: &str, candidate: &str, sequence: u64) -> DifferentialVector {
    DifferentialVector {
        vector_id: id.to_owned(),
        reference_outcome: reference.to_owned(),
        candidate_outcome: candidate.to_owned(),
        evidence_refs: BTreeSet::from([format!("evidence-{id}")]),
        logical_sequence: sequence,
    }
}

fn input(vectors: Vec<DifferentialVector>) -> DifferentialVectorSetInput {
    DifferentialVectorSetInput {
        vector_set_id: "reducers-v1".to_owned(),
        comparison_mode: "exact".to_owned(),
        comparison_profile_revision: "cmp-v43".to_owned(),
        vectors,
    }
}

#[test]
fn all_equivalent_vectors_measure_crdr_without_minting_a_divergence() {
    let record = adapt_differential_vector_set(input(vec![
        vector("v1", "digest:a", "digest:a", 101),
        vector("v2", "digest:b", "digest:b", 102),
    ]))
    .expect("valid differential vector set");

    assert_eq!(
        record.result_evidence,
        Some(ResultEvidence::DifferentialEquivalentWithinVectorSet)
    );
    assert_eq!(record.first_divergence_vector_id, None);
    assert_eq!(record.observations.len(), 2);
    assert!(
        record
            .observations
            .iter()
            .all(|observation| observation.failure_flags.is_empty())
    );

    let metrics = reduce_metric_observations(&record.observations).expect("reduce CRDR");
    let crdr = metrics.get(LabMetricKind::Crdr).expect("CRDR present");
    assert_eq!(crdr.status, MetricStatus::Measured);
    assert_eq!(crdr.numerator, 0);
    assert_eq!(crdr.denominator, 2);
}

#[test]
fn divergence_is_derived_from_outputs_and_preserves_first_vector_identity() {
    let record = adapt_differential_vector_set(input(vec![
        vector("v1", "digest:a", "digest:a", 201),
        vector("v2", "digest:b", "digest:x", 202),
        vector("v3", "digest:c", "digest:y", 203),
    ]))
    .expect("valid divergent vector set");

    assert_eq!(
        record.result_evidence,
        Some(ResultEvidence::DifferentialDivergenceFound)
    );
    assert_eq!(record.first_divergence_vector_id.as_deref(), Some("v2"));
    assert_eq!(
        record.observations[1].failure_flags,
        BTreeSet::from([LabFailureFlag::CrossReducerDivergence])
    );

    let metrics = reduce_metric_observations(&record.observations).expect("reduce CRDR");
    let crdr = metrics.get(LabMetricKind::Crdr).expect("CRDR present");
    assert_eq!(crdr.numerator, 2);
    assert_eq!(crdr.denominator, 3);
}

#[test]
fn empty_vector_set_is_not_measured_and_cannot_mint_equivalence() {
    let record = adapt_differential_vector_set(input(Vec::new()))
        .expect("empty differential campaign is typed non-measurement");

    assert_eq!(record.result_evidence, None);
    assert_eq!(record.first_divergence_vector_id, None);
    assert!(record.observations.is_empty());

    let metrics = reduce_metric_observations(&record.observations).expect("reduce empty campaign");
    let crdr = metrics.get(LabMetricKind::Crdr).expect("CRDR present");
    assert_eq!(crdr.status, MetricStatus::NotMeasured);
    assert_eq!(crdr.denominator, 0);
    assert_eq!(crdr.rate_ppb, None);
}

#[test]
fn differential_authority_fails_closed_before_observations_are_minted() {
    let unsupported = DifferentialVectorSetInput {
        comparison_mode: "normalized".to_owned(),
        ..input(vec![vector("v1", "a", "a", 301)])
    };
    assert_eq!(
        adapt_differential_vector_set(unsupported).expect_err("unsupported mode must fail"),
        LabError::UnsupportedDifferentialComparisonMode {
            mode: "normalized".to_owned()
        }
    );

    let empty_revision = DifferentialVectorSetInput {
        comparison_profile_revision: "   ".to_owned(),
        ..input(vec![vector("v1", "a", "a", 302)])
    };
    assert_eq!(
        adapt_differential_vector_set(empty_revision)
            .expect_err("empty comparison authority must fail"),
        LabError::EmptyAuthorityField {
            field: "comparison_profile_revision"
        }
    );

    let duplicate = input(vec![
        vector("same", "a", "a", 303),
        vector("same", "b", "b", 304),
    ]);
    assert_eq!(
        adapt_differential_vector_set(duplicate).expect_err("duplicate vector IDs must fail"),
        LabError::DuplicateDifferentialVectorId {
            vector_id: "same".to_owned()
        }
    );
}

#[test]
fn empty_differential_identities_fail_closed() {
    let empty_set = DifferentialVectorSetInput {
        vector_set_id: "   ".to_owned(),
        ..input(vec![vector("v1", "a", "a", 401)])
    };
    assert_eq!(
        adapt_differential_vector_set(empty_set).expect_err("empty vector-set identity must fail"),
        LabError::EmptyAuthorityField {
            field: "vector_set_id"
        }
    );

    let empty_vector = input(vec![vector("   ", "a", "a", 402)]);
    assert_eq!(
        adapt_differential_vector_set(empty_vector).expect_err("empty vector identity must fail"),
        LabError::EmptyAuthorityField { field: "vector_id" }
    );
}
