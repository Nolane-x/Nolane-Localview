use localview_validation_lab::{
    LabError, MetamorphicRelation, ResultEvidence, adapt_metamorphic_case,
};

#[test]
fn equality_relation_passes_only_when_generated_pair_is_invariant() {
    let pass = adapt_metamorphic_case(
        "case-canonical-order-1",
        "canonicalization-order-independence",
        "digest:abc",
        "digest:abc",
        MetamorphicRelation::Equal,
        ["evidence-b", "evidence-a", "evidence-a"],
        "cmp-v43",
        41,
    )
    .expect("valid L2 metamorphic case");

    assert_eq!(pass.result_evidence, ResultEvidence::PreregisteredSeedPass);
    assert!(pass.relation_satisfied);
    assert_eq!(
        pass.observation.evidence_refs.into_iter().collect::<Vec<_>>(),
        vec!["evidence-a".to_owned(), "evidence-b".to_owned()]
    );

    let fail = adapt_metamorphic_case(
        "case-canonical-order-2",
        "canonicalization-order-independence",
        "digest:abc",
        "digest:def",
        MetamorphicRelation::Equal,
        std::iter::empty::<&str>(),
        "cmp-v43",
        42,
    )
    .expect("valid failing L2 metamorphic case");

    assert_eq!(fail.result_evidence, ResultEvidence::CounterexampleFound);
    assert!(!fail.relation_satisfied);
}

#[test]
fn inequality_relation_is_evaluated_by_the_adapter_not_a_caller_verdict() {
    let pass = adapt_metamorphic_case(
        "case-non-aliasing-1",
        "freshness-token-non-aliasing",
        "token:a",
        "token:b",
        MetamorphicRelation::NotEqual,
        ["trace-1"],
        "cmp-v43",
        51,
    )
    .expect("valid non-aliasing case");

    assert!(pass.relation_satisfied);
    assert_eq!(pass.result_evidence, ResultEvidence::PreregisteredSeedPass);
}

#[test]
fn authority_fields_fail_closed_before_an_observation_can_exist() {
    let error = adapt_metamorphic_case(
        "case-1",
        "   ",
        "a",
        "a",
        MetamorphicRelation::Equal,
        std::iter::empty::<&str>(),
        "cmp-v43",
        61,
    )
    .expect_err("empty property authority must fail closed");

    assert_eq!(
        error,
        LabError::EmptyAuthorityField {
            field: "property_name"
        }
    );
}
