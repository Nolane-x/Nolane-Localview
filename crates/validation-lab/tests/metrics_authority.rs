use localview_validation_lab::{
    LabMetricKind, LabMetricValue, MetricSnapshot, MetricStatus,
    SilentUnsoundnessGateStatus, evaluate_silent_unsoundness_gate,
};

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
