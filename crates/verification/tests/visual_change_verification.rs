use localview_protocol::{VisualChangeExpectation, VisualDiffMetrics};
use localview_verification::{verify_visual_change, LiveVerificationVerdict};

fn metrics(changed_pixels: u64, changed_ratio: f64) -> VisualDiffMetrics {
    VisualDiffMetrics {
        changed_pixels,
        changed_ratio,
    }
}

#[test]
fn unchanged_expectation_passes_only_inside_the_maximum_ratio() {
    let expectation = VisualChangeExpectation::Unchanged {
        max_changed_ratio: 0.01,
    };

    let pass = verify_visual_change(&expectation, &metrics(4, 0.004));
    assert_eq!(pass.verdict, LiveVerificationVerdict::Pass);
    assert_eq!(pass.changed_pixels, 4);
    assert_eq!(pass.changed_ratio, 0.004);

    let fail = verify_visual_change(&expectation, &metrics(30, 0.03));
    assert_eq!(fail.verdict, LiveVerificationVerdict::Fail);
    assert!(fail.reason.contains("maximum"));
}

#[test]
fn changed_expectation_passes_only_at_or_above_the_minimum_ratio() {
    let expectation = VisualChangeExpectation::Changed {
        min_changed_ratio: 0.02,
    };

    let pass = verify_visual_change(&expectation, &metrics(25, 0.025));
    assert_eq!(pass.verdict, LiveVerificationVerdict::Pass);

    let fail = verify_visual_change(&expectation, &metrics(5, 0.005));
    assert_eq!(fail.verdict, LiveVerificationVerdict::Fail);
    assert!(fail.reason.contains("minimum"));
}

#[test]
fn invalid_expectations_and_metrics_fail_closed_as_inconclusive() {
    for expectation in [
        VisualChangeExpectation::Unchanged {
            max_changed_ratio: f64::NAN,
        },
        VisualChangeExpectation::Changed {
            min_changed_ratio: 1.1,
        },
    ] {
        let decision = verify_visual_change(&expectation, &metrics(0, 0.0));
        assert_eq!(decision.verdict, LiveVerificationVerdict::Inconclusive);
        assert!(decision.reason.contains("invalid"));
    }

    for ratio in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let decision = verify_visual_change(
            &VisualChangeExpectation::Changed {
                min_changed_ratio: 0.01,
            },
            &metrics(1, ratio),
        );
        assert_eq!(decision.verdict, LiveVerificationVerdict::Inconclusive);
        assert!(decision.reason.contains("invalid"));
    }
}
