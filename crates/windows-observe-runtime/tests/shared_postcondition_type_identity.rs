use localview_postcondition_contracts::{
    NativeSemanticNodeMatcherV1 as SharedNodeMatcherV1,
    NativeSemanticPostconditionContractV1 as SharedContractV1,
    NativeSemanticPostconditionExpectation as SharedExpectation,
};
use localview_windows_observe_runtime::{
    NativeSemanticNodeMatcherV1 as WindowsNodeMatcherV1,
    NativeSemanticPostconditionContractV1 as WindowsContractV1,
    NativeSemanticPostconditionExpectation as WindowsExpectation,
};

#[test]
fn windows_public_postcondition_types_are_the_shared_correctness_types() {
    let shared = SharedContractV1 {
        expectation: SharedExpectation::Present,
        matcher: SharedNodeMatcherV1 {
            name: Some("Completed".into()),
            ..Default::default()
        },
    };

    let windows: WindowsContractV1 = shared;
    let shared_again: SharedContractV1 = windows;
    assert_eq!(shared_again.expectation, SharedExpectation::Present);

    let windows_expectation: WindowsExpectation = SharedExpectation::Absent;
    let shared_expectation: SharedExpectation = windows_expectation;
    assert_eq!(shared_expectation, SharedExpectation::Absent);

    let windows_matcher: WindowsNodeMatcherV1 = SharedNodeMatcherV1 {
        automation_id: Some("completion-status".into()),
        ..Default::default()
    };
    let shared_matcher: SharedNodeMatcherV1 = windows_matcher;
    assert_eq!(
        shared_matcher.automation_id.as_deref(),
        Some("completion-status")
    );
}
