use localview_validation_lab::{
    LabError, MetamorphicCaseInput, MetamorphicRelation, ResultEvidence, adapt_metamorphic_case,
};

#[test]
fn equality_relation_passes_only_when_generated_pair_is_invariant() {
    let pass = adapt_metamorphic_case(MetamorphicCaseInput {
        case_id: "case-canonical-order-1",
        property_name: "canonicalization-order-independence",
        base_outcome: "digest:abc",
        transformed_outcome: "digest:abc",
        relation: MetamorphicRelation::Equal,
        evidence_refs: ["evidence-b", "evidence-a", "evidence-a"],
        comparison_profile_revision: "cmp-v43",
        logical_sequence: 41,
    })
    .expect("valid L2 metamorphic case");

    assert_eq!(pass.result_evidence, ResultEvidence::PreregisteredSeedPass);
    assert!(pass.relation_satisfied);
    assert_eq!(
        pass.observation
            .evidence_refs
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["evidence-a".to_owned(), "evidence-b".to_owned()]
    );

    let fail = adapt_metamorphic_case(MetamorphicCaseInput {
        case_id: "case-canonical-order-2",
        property_name: "canonicalization-order-independence",
        base_outcome: "digest:abc",
        transformed_outcome: "digest:def",
        relation: MetamorphicRelation::Equal,
        evidence_refs: std::iter::empty::<&str>(),
        comparison_profile_revision: "cmp-v43",
        logical_sequence: 42,
    })
    .expect("valid failing L2 metamorphic case");

    assert_eq!(fail.result_evidence, ResultEvidence::CounterexampleFound);
    assert!(!fail.relation_satisfied);
}

#[test]
fn inequality_relation_is_evaluated_by_the_adapter_not_a_caller_verdict() {
    let pass = adapt_metamorphic_case(MetamorphicCaseInput {
        case_id: "case-non-aliasing-1",
        property_name: "freshness-token-non-aliasing",
        base_outcome: "token:a",
        transformed_outcome: "token:b",
        relation: MetamorphicRelation::NotEqual,
        evidence_refs: ["trace-1"],
        comparison_profile_revision: "cmp-v43",
        logical_sequence: 51,
    })
    .expect("valid non-aliasing case");

    assert!(pass.relation_satisfied);
    assert_eq!(pass.result_evidence, ResultEvidence::PreregisteredSeedPass);
}

#[test]
fn authority_fields_fail_closed_before_an_observation_can_exist() {
    let error = adapt_metamorphic_case(MetamorphicCaseInput {
        case_id: "case-1",
        property_name: "   ",
        base_outcome: "a",
        transformed_outcome: "a",
        relation: MetamorphicRelation::Equal,
        evidence_refs: std::iter::empty::<&str>(),
        comparison_profile_revision: "cmp-v43",
        logical_sequence: 61,
    })
    .expect_err("empty property authority must fail closed");

    assert_eq!(
        error,
        LabError::EmptyAuthorityField {
            field: "property_name"
        }
    );
}
