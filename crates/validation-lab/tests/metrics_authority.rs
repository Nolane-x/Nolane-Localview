use localview_validation_lab::{
    LabError, LabMetricKind, LabMetricValue, MetricSnapshot, MetricStatus,
    SilentUnsoundnessGateStatus, evaluate_silent_unsoundness_gate,
};

fn set_required_zero_metrics(snapshot: &mut MetricSnapshot) {
    for kind in LabMetricKind::SILENT_UNSOUNDNESS_ZERO_TARGET {
        snapshot.set(LabMetricValue::new(kind, 0, 1).unwrap());
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
