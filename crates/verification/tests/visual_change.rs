use localview_verification::{
    verify_visual_change, VisualChangeExpectation, VisualChangeObservation, VisualChangeVerdict,
};

#[test]
fn baseline_reset_is_inconclusive_even_when_pixels_are_available() {
    let observation = VisualChangeObservation {
        changed_ratio: 1.0,
        baseline_comparable: false,
    };

    let result = verify_visual_change(
        &observation,
        VisualChangeExpectation::Unchanged {
            max_changed_ratio: 0.001,
        },
    )
    .expect("valid assertion policy");

    assert_eq!(result.verdict, VisualChangeVerdict::Inconclusive);
    assert_eq!(result.changed_ratio, 1.0);
    assert!(result.reason.contains("comparable baseline"));
}

#[test]
fn unchanged_expectation_passes_and_fails_deterministically_at_the_boundary() {
    let expectation = VisualChangeExpectation::Unchanged {
        max_changed_ratio: 0.01,
    };

    let pass = verify_visual_change(
        &VisualChangeObservation {
            changed_ratio: 0.01,
            baseline_comparable: true,
        },
        expectation,
    )
    .expect("valid assertion policy");
    assert_eq!(pass.verdict, VisualChangeVerdict::Pass);

    let fail = verify_visual_change(
        &VisualChangeObservation {
            changed_ratio: 0.010_001,
            baseline_comparable: true,
        },
        expectation,
    )
    .expect("valid assertion policy");
    assert_eq!(fail.verdict, VisualChangeVerdict::Fail);
}

#[test]
fn changed_expectation_passes_and_fails_deterministically_at_the_boundary() {
    let expectation = VisualChangeExpectation::Changed {
        min_changed_ratio: 0.2,
    };

    let pass = verify_visual_change(
        &VisualChangeObservation {
            changed_ratio: 0.2,
            baseline_comparable: true,
        },
        expectation,
    )
    .expect("valid assertion policy");
    assert_eq!(pass.verdict, VisualChangeVerdict::Pass);

    let fail = verify_visual_change(
        &VisualChangeObservation {
            changed_ratio: 0.199_999,
            baseline_comparable: true,
        },
        expectation,
    )
    .expect("valid assertion policy");
    assert_eq!(fail.verdict, VisualChangeVerdict::Fail);
}

#[test]
fn invalid_or_non_finite_ratios_fail_closed() {
    for changed_ratio in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        assert!(verify_visual_change(
            &VisualChangeObservation {
                changed_ratio,
                baseline_comparable: true,
            },
            VisualChangeExpectation::Unchanged {
                max_changed_ratio: 0.01,
            },
        )
        .is_err());
    }

    for max_changed_ratio in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        assert!(verify_visual_change(
            &VisualChangeObservation {
                changed_ratio: 0.0,
                baseline_comparable: true,
            },
            VisualChangeExpectation::Unchanged { max_changed_ratio },
        )
        .is_err());
    }

    for min_changed_ratio in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        assert!(verify_visual_change(
            &VisualChangeObservation {
                changed_ratio: 0.5,
                baseline_comparable: true,
            },
            VisualChangeExpectation::Changed { min_changed_ratio },
        )
        .is_err());
    }
}
