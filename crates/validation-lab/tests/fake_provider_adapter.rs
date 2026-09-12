use std::collections::BTreeSet;

use localview_protocol::{
    EventContinuityState, PrincipalRef, ProviderIncarnationRef, ReconciliationCompleteness,
};
use localview_validation_lab::{
    FakeProviderCaseInput, FakeProviderScenario, LabError, LabFailureFlag, LabMetricKind,
    ResultEvidence, adapt_fake_provider_case, reduce_metric_observations,
};

fn refs(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn freshness_and_reconciliation_failures_are_typed_and_conservative() {
    let false_fresh = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "fresh-gap",
        scenario: FakeProviderScenario::Freshness {
            continuity: EventContinuityState::GapDetected,
            reconciliation: None,
            accepted_as_fresh: true,
        },
        evidence_refs: refs(&["fake:event-gap"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 501,
    })
    .unwrap();
    assert_eq!(
        false_fresh.result_evidence,
        ResultEvidence::CounterexampleFound
    );
    assert_eq!(
        false_fresh.observation.eligible_metrics,
        BTreeSet::from([LabMetricKind::Eoffr])
    );
    assert_eq!(
        false_fresh.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::EventOnlyFalseFreshness])
    );
    assert!(false_fresh.observation.provider_backed);

    let reconciled_fresh = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "fresh-after-reconciliation",
        scenario: FakeProviderScenario::Freshness {
            continuity: EventContinuityState::GapDetected,
            reconciliation: Some(ReconciliationCompleteness::Established),
            accepted_as_fresh: true,
        },
        evidence_refs: refs(&["fake:reconciled"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 502,
    })
    .unwrap();
    assert!(reconciled_fresh.observation.failure_flags.is_empty());
    assert_eq!(
        reconciled_fresh.result_evidence,
        ResultEvidence::PreregisteredSeedPass
    );

    let conservative_block = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "opaque-blocked",
        scenario: FakeProviderScenario::Freshness {
            continuity: EventContinuityState::OrderingOpaque,
            reconciliation: None,
            accepted_as_fresh: false,
        },
        evidence_refs: refs(&["fake:opaque"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 503,
    })
    .unwrap();
    assert!(conservative_block.observation.failure_flags.is_empty());

    let reconciliation_miss = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "reconciliation-miss",
        scenario: FakeProviderScenario::Reconciliation {
            completeness: ReconciliationCompleteness::Incomplete,
            accepted_as_reconciled: true,
        },
        evidence_refs: refs(&["fake:incomplete-reconciliation"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 504,
    })
    .unwrap();
    assert_eq!(
        reconciliation_miss.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::ReconciliationMiss])
    );

    let snapshot = reduce_metric_observations(&[
        false_fresh.observation,
        reconciled_fresh.observation,
        conservative_block.observation,
        reconciliation_miss.observation,
    ])
    .unwrap();
    let eoffr = snapshot.get(LabMetricKind::Eoffr).unwrap();
    assert_eq!((eoffr.numerator, eoffr.denominator), (1, 3));
    let rmr = snapshot.get(LabMetricKind::Rmr).unwrap();
    assert_eq!((rmr.numerator, rmr.denominator), (1, 1));
}

#[test]
fn provider_aba_escape_requires_real_reincarnation_and_is_counted_once() {
    let escaped = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "aba-escape",
        scenario: FakeProviderScenario::ProviderAba {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:fake:old"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:fake:new"),
            opaque_provider_element_id: "node-7".into(),
            accepted_previous_identity_as_current: true,
        },
        evidence_refs: refs(&["fake:aba"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 505,
    })
    .unwrap();
    assert_eq!(
        escaped.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::ProviderIdAbaEscape])
    );

    let rejected = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "aba-rejected",
        scenario: FakeProviderScenario::ProviderAba {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:fake:old"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:fake:new"),
            opaque_provider_element_id: "node-7".into(),
            accepted_previous_identity_as_current: false,
        },
        evidence_refs: refs(&["fake:aba-rejected"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 506,
    })
    .unwrap();
    assert!(rejected.observation.failure_flags.is_empty());

    let snapshot =
        reduce_metric_observations(&[escaped.observation, rejected.observation]).unwrap();
    let piaer = snapshot.get(LabMetricKind::Piaer).unwrap();
    assert_eq!((piaer.numerator, piaer.denominator), (1, 2));

    let same_incarnation = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "not-an-aba-challenge",
        scenario: FakeProviderScenario::ProviderAba {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:fake:same"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:fake:same"),
            opaque_provider_element_id: "node-7".into(),
            accepted_previous_identity_as_current: true,
        },
        evidence_refs: BTreeSet::new(),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 507,
    });
    assert_eq!(
        same_incarnation,
        Err(LabError::InvalidFakeProviderScenario {
            reason: "provider_aba_requires_distinct_incarnations"
        })
    );
}

#[test]
fn principal_dispatch_and_visibility_map_to_wpdr_and_pilr_without_string_inference() {
    let wrong_dispatch = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "wrong-principal",
        scenario: FakeProviderScenario::PrincipalDispatch {
            expected_principal: PrincipalRef::from("principal:expected"),
            dispatched_principal: PrincipalRef::from("principal:other"),
        },
        evidence_refs: refs(&["fake:dispatch"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 508,
    })
    .unwrap();
    assert_eq!(
        wrong_dispatch.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::WrongPrincipalDispatch])
    );
    assert_eq!(
        wrong_dispatch.observation.principal_expected.as_deref(),
        Some("principal:expected")
    );
    assert_eq!(
        wrong_dispatch.observation.principal_dispatched.as_deref(),
        Some("principal:other")
    );

    let correct_dispatch = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "correct-principal",
        scenario: FakeProviderScenario::PrincipalDispatch {
            expected_principal: PrincipalRef::from("principal:expected"),
            dispatched_principal: PrincipalRef::from("principal:expected"),
        },
        evidence_refs: refs(&["fake:dispatch-ok"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 509,
    })
    .unwrap();
    assert!(correct_dispatch.observation.failure_flags.is_empty());

    let leaked = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "principal-leak",
        scenario: FakeProviderScenario::PrincipalVisibility {
            authorized_principal: PrincipalRef::from("principal:owner"),
            exposed_principal: PrincipalRef::from("principal:foreign"),
        },
        evidence_refs: refs(&["fake:visibility"]),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 510,
    })
    .unwrap();
    assert_eq!(
        leaked.observation.failure_flags,
        BTreeSet::from([LabFailureFlag::PrincipalInformationLeak])
    );

    let snapshot = reduce_metric_observations(&[
        wrong_dispatch.observation,
        correct_dispatch.observation,
        leaked.observation,
    ])
    .unwrap();
    let wpdr = snapshot.get(LabMetricKind::Wpdr).unwrap();
    assert_eq!((wpdr.numerator, wpdr.denominator), (1, 2));
    let pilr = snapshot.get(LabMetricKind::Pilr).unwrap();
    assert_eq!((pilr.numerator, pilr.denominator), (1, 1));
}

#[test]
fn fake_provider_authority_fields_fail_closed_before_observation_minting() {
    let empty_case = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "   ",
        scenario: FakeProviderScenario::Freshness {
            continuity: EventContinuityState::Continuous,
            reconciliation: None,
            accepted_as_fresh: true,
        },
        evidence_refs: BTreeSet::new(),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 511,
    });
    assert_eq!(
        empty_case,
        Err(LabError::EmptyAuthorityField { field: "case_id" })
    );

    let empty_profile = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "missing-profile",
        scenario: FakeProviderScenario::Freshness {
            continuity: EventContinuityState::Continuous,
            reconciliation: None,
            accepted_as_fresh: true,
        },
        evidence_refs: BTreeSet::new(),
        comparison_profile_revision: " ",
        logical_sequence: 512,
    });
    assert_eq!(
        empty_profile,
        Err(LabError::EmptyAuthorityField {
            field: "comparison_profile_revision"
        })
    );

    let empty_opaque_id = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "empty-provider-id",
        scenario: FakeProviderScenario::ProviderAba {
            previous_provider_incarnation: ProviderIncarnationRef::from("provider:fake:old"),
            current_provider_incarnation: ProviderIncarnationRef::from("provider:fake:new"),
            opaque_provider_element_id: " ".into(),
            accepted_previous_identity_as_current: false,
        },
        evidence_refs: BTreeSet::new(),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 513,
    });
    assert_eq!(
        empty_opaque_id,
        Err(LabError::EmptyAuthorityField {
            field: "opaque_provider_element_id"
        })
    );

    let empty_principal = adapt_fake_provider_case(FakeProviderCaseInput {
        case_id: "empty-principal",
        scenario: FakeProviderScenario::PrincipalDispatch {
            expected_principal: PrincipalRef::from(" "),
            dispatched_principal: PrincipalRef::from("principal:other"),
        },
        evidence_refs: BTreeSet::new(),
        comparison_profile_revision: "fake-provider-v1",
        logical_sequence: 514,
    });
    assert_eq!(
        empty_principal,
        Err(LabError::EmptyAuthorityField {
            field: "expected_principal"
        })
    );
}
