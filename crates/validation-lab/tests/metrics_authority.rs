use std::collections::BTreeSet;

use localview_validation_lab::{
    LabError, LabFailureFlag, LabMetricKind, LabMetricValue, LabObservation, MetricSnapshot,
    MetricStatus, SilentUnsoundnessGateStatus, evaluate_silent_unsoundness_gate,
    reduce_metric_observations,
};

fn set_required_zero_metrics(snapshot: &mut MetricSnapshot) {
    for kind in LabMetricKind::SILENT_UNSOUNDNESS_ZERO_TARGET {
        snapshot.set(LabMetricValue::new(kind, 0, 1).unwrap());
    }
}

fn observation(
    observation_id: &str,
    eligible_metrics: BTreeSet<LabMetricKind>,
    failure_flags: BTreeSet<LabFailureFlag>,
    observed_outcome: &str,
) -> LabObservation {
    LabObservation {
        observation_id: observation_id.into(),
        seed_id: Some("LV-S041".into()),
        expected_outcome: "CURRENT".into(),
        observed_outcome: observed_outcome.into(),
        principal_expected: Some("principal-a".into()),
        principal_dispatched: Some("principal-a".into()),
        eligible_metrics,
        failure_flags,
        evidence_refs: BTreeSet::from([format!("evidence:{observation_id}")]),
        provider_backed: false,
        comparison_profile_revision: "compare-r7".into(),
        logical_sequence: 21,
    }
}

#[test]
fn v43_metric_catalog_is_complete_and_empty_evidence_cannot_pass() {
    assert_eq!(LabMetricKind::ALL.len(), 14);

    let empty = MetricSnapshot::empty();
    for kind in LabMetricKind::ALL {
        let value = empty.get(kind).expect("every V4.3 metric is represented");
        assert_eq!(value.denominator, 0);
        assert_eq!(value.status, MetricStatus::NotMeasured);
        assert_eq!(value.rate_ppb, None);
    }

    assert_eq!(
        evaluate_silent_unsoundness_gate(&empty),
        SilentUnsoundnessGateStatus::Incomplete,
        "an empty campaign is not evidence that the zero-target gate passed",
    );
}

#[test]
fn metric_value_api_is_explicit_about_zero_denominator() {
    let value = LabMetricValue::new(LabMetricKind::Suar, 0, 0).unwrap();
    assert_eq!(value.status, MetricStatus::NotMeasured);
    assert_eq!(value.rate_ppb, None);
}

#[test]
fn one_wrong_principal_dispatch_is_one_of_one_and_fails_gate() {
    let mut snapshot = MetricSnapshot::empty();
    set_required_zero_metrics(&mut snapshot);
    snapshot.set(LabMetricValue::new(LabMetricKind::Wpdr, 1, 1).unwrap());

    let wpdr = snapshot.get(LabMetricKind::Wpdr).unwrap();
    assert_eq!(wpdr.numerator, 1);
    assert_eq!(wpdr.denominator, 1);
    assert_eq!(wpdr.status, MetricStatus::Measured);
    assert_eq!(wpdr.rate_ppb, Some(1_000_000_000));
    assert_eq!(
        evaluate_silent_unsoundness_gate(&snapshot),
        SilentUnsoundnessGateStatus::Fail,
    );
}

#[test]
fn all_required_zero_metrics_pass_only_when_measured() {
    let mut snapshot = MetricSnapshot::empty();
    set_required_zero_metrics(&mut snapshot);
    assert_eq!(
        evaluate_silent_unsoundness_gate(&snapshot),
        SilentUnsoundnessGateStatus::Pass,
    );

    snapshot.set(LabMetricValue::new(LabMetricKind::Uobrr, 0, 0).unwrap());
    assert_eq!(
        evaluate_silent_unsoundness_gate(&snapshot),
        SilentUnsoundnessGateStatus::Incomplete,
    );
}

#[test]
fn metric_numerator_must_be_a_subset_of_denominator() {
    assert_eq!(
        LabMetricValue::new(LabMetricKind::Pdmr, 2, 1),
        Err(LabError::InvalidMetricSubset {
            numerator: 2,
            denominator: 1,
        }),
    );
}

#[test]
fn every_failure_flag_maps_to_exactly_one_v43_metric() {
    let mappings = [
        (LabFailureFlag::SilentUnsoundAction, LabMetricKind::Suar),
        (LabFailureFlag::WrongPrincipalDispatch, LabMetricKind::Wpdr),
        (
            LabFailureFlag::PrincipalInformationLeak,
            LabMetricKind::Pilr,
        ),
        (
            LabFailureFlag::EventOnlyFalseFreshness,
            LabMetricKind::Eoffr,
        ),
        (LabFailureFlag::ReconciliationMiss, LabMetricKind::Rmr),
        (LabFailureFlag::ProviderIdAbaEscape, LabMetricKind::Piaer),
        (LabFailureFlag::WrongForegroundInput, LabMetricKind::Wfir),
        (
            LabFailureFlag::PartialDispatchMisclassifiedSuccess,
            LabMetricKind::Pdmr,
        ),
        (LabFailureFlag::StaleCacheAuthority, LabMetricKind::Scar),
        (LabFailureFlag::BlindRetryAfterUnknown, LabMetricKind::Uobrr),
        (LabFailureFlag::MutationSurvived, LabMetricKind::Msr),
        (LabFailureFlag::CrossReducerDivergence, LabMetricKind::Crdr),
        (
            LabFailureFlag::RealProviderOracleMismatch,
            LabMetricKind::Rpomr,
        ),
        (
            LabFailureFlag::CleanupToBaselineFailure,
            LabMetricKind::Cbfr,
        ),
    ];

    let observations = mappings
        .iter()
        .enumerate()
        .map(|(index, (flag, metric))| {
            observation(
                &format!("obs-{index}"),
                BTreeSet::from([*metric]),
                BTreeSet::from([*flag]),
                "UNSOUND",
            )
        })
        .collect::<Vec<_>>();

    let snapshot = reduce_metric_observations(&observations).unwrap();
    for (_, metric) in mappings {
        let value = snapshot.get(metric).unwrap();
        assert_eq!(
            (value.numerator, value.denominator),
            (1, 1),
            "wrong mapping for {metric:?}"
        );
        assert_eq!(value.status, MetricStatus::Measured);
    }
}

#[test]
fn conservative_inconclusive_without_unsound_flag_is_zero_of_one_suar() {
    let observation = observation(
        "inconclusive",
        BTreeSet::from([LabMetricKind::Suar]),
        BTreeSet::new(),
        "INCONCLUSIVE",
    );
    let snapshot = reduce_metric_observations(&[observation]).unwrap();
    let suar = snapshot.get(LabMetricKind::Suar).unwrap();

    assert_eq!((suar.numerator, suar.denominator), (0, 1));
    assert_eq!(suar.status, MetricStatus::Measured);
}

#[test]
fn failure_flag_without_matching_eligibility_is_a_typed_error() {
    let observation = observation(
        "invalid-eligibility",
        BTreeSet::from([LabMetricKind::Suar]),
        BTreeSet::from([LabFailureFlag::WrongPrincipalDispatch]),
        "WRONG_PRINCIPAL",
    );

    assert_eq!(
        reduce_metric_observations(&[observation]),
        Err(LabError::FailureFlagWithoutEligibility {
            flag: LabFailureFlag::WrongPrincipalDispatch,
            metric: LabMetricKind::Wpdr,
        }),
    );
}

#[test]
fn reduced_zero_target_evidence_drives_pass_fail_and_incomplete() {
    let required = LabMetricKind::SILENT_UNSOUNDNESS_ZERO_TARGET;
    let all_zero = required
        .into_iter()
        .enumerate()
        .map(|(index, metric)| {
            observation(
                &format!("required-zero-{index}"),
                BTreeSet::from([metric]),
                BTreeSet::new(),
                "SAFE",
            )
        })
        .collect::<Vec<_>>();
    let pass_snapshot = reduce_metric_observations(&all_zero).unwrap();
    assert_eq!(
        evaluate_silent_unsoundness_gate(&pass_snapshot),
        SilentUnsoundnessGateStatus::Pass,
    );

    let mut failing = all_zero.clone();
    failing.push(observation(
        "blind-retry",
        BTreeSet::from([LabMetricKind::Uobrr]),
        BTreeSet::from([LabFailureFlag::BlindRetryAfterUnknown]),
        "BLIND_RETRY",
    ));
    let fail_snapshot = reduce_metric_observations(&failing).unwrap();
    assert_eq!(
        evaluate_silent_unsoundness_gate(&fail_snapshot),
        SilentUnsoundnessGateStatus::Fail,
    );

    let incomplete_snapshot = reduce_metric_observations(&all_zero[..4]).unwrap();
    assert_eq!(
        evaluate_silent_unsoundness_gate(&incomplete_snapshot),
        SilentUnsoundnessGateStatus::Incomplete,
    );
}
