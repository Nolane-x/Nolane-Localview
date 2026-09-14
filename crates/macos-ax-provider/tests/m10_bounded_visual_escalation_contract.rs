use localview_macos_ax_provider::{
    AxApplicationIncarnation, AxBoundedVisualEscalationAuthority, AxElementBindingProvider,
    AxElementIdentity, AxElementOperationDecision, AxVisualEscalationAuthorizationError,
    AxVisualEscalationReason, VisualObservationPermissionError,
    VisualObservationPermissionProvider, VisualObservationPermissionState,
};
use localview_resource_governor::{
    ResourceBudget, ResourceWorkKind, RuntimeResourceGovernor, RuntimeResourceSample,
};

const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;

fn identity() -> AxElementIdentity {
    AxElementIdentity::new(
        AxApplicationIncarnation::new("com.nolane.localview.m10-seed", 4545, 1),
        "window:localview-m10-seed",
        "ax-identifier:localview-m10-target",
    )
}

fn timeout_request() -> localview_macos_ax_provider::AxBoundedVisualEscalationRequest {
    let binding_provider = AxElementBindingProvider::new();
    let binding = binding_provider.bind_current(identity());
    let unresponsive = match binding_provider.observe_operation(binding, AX_ERROR_CANNOT_COMPLETE) {
        AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
        other => panic!("expected timeout/unresponsive tombstone, got {other:?}"),
    };
    AxBoundedVisualEscalationAuthority::new().request_after_unresponsive(unresponsive)
}

#[test]
fn timeout_creates_only_a_bounded_visual_escalation_request_not_capture_authority() {
    let binding_provider = AxElementBindingProvider::new();
    let binding = binding_provider.bind_current(identity());
    let invalidated_sequence = binding.binding_sequence();
    let unresponsive = match binding_provider.observe_operation(binding, AX_ERROR_CANNOT_COMPLETE) {
        AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
        other => panic!("expected timeout/unresponsive tombstone, got {other:?}"),
    };

    let request = AxBoundedVisualEscalationAuthority::new()
        .request_after_unresponsive(unresponsive);

    assert_eq!(request.reason(), AxVisualEscalationReason::ProviderTimeout);
    assert_eq!(request.invalidated_binding_sequence(), invalidated_sequence);
    assert_eq!(request.max_visual_regions(), 1);
    assert!(!request.visual_capture_permitted());
    assert!(!request.input_fallback_permitted());
}

#[test]
fn visual_escalation_requires_both_visual_permission_and_governor_admission() {
    let mut budget = ResourceBudget::default();
    budget.concurrent_captures = 1;
    let governor = RuntimeResourceGovernor::new(budget);
    let visual = VisualObservationPermissionProvider::validation_decision_for_state(
        VisualObservationPermissionState::Granted,
    );

    let permit = AxBoundedVisualEscalationAuthority::new()
        .authorize_visual_observation(
            timeout_request(),
            visual,
            &governor,
            "m10-session",
            "m10-timeout-visual",
        )
        .expect("explicit permission plus governor admission must mint one bounded visual permit");

    assert!(permit.visual_capture_permitted());
    assert!(!permit.input_fallback_permitted());
    assert_eq!(permit.max_visual_regions(), 1);
    assert_eq!(permit.reason(), AxVisualEscalationReason::ProviderTimeout);
    assert_eq!(permit.target_identity(), &identity());
    assert!(permit.visual_permission_check_sequence() > 0);

    assert!(
        governor
            .reserve(
                "m10-session",
                "parallel-capture",
                ResourceWorkKind::NativeVisualCapture,
            )
            .is_err(),
        "the bounded permit must retain its governor reservation for the capture lifetime"
    );

    drop(permit);
    assert!(
        governor
            .reserve(
                "m10-session",
                "after-release",
                ResourceWorkKind::NativeVisualCapture,
            )
            .is_ok(),
        "dropping the permit must release the bounded capture reservation"
    );
}

#[test]
fn resource_pressure_denial_preserves_timeout_request_for_semantic_reconciliation() {
    let governor = RuntimeResourceGovernor::default();
    assert!(governor.update_sample(RuntimeResourceSample {
        memory_mb: 1024,
        cpu_percent: 0.0,
        capture_storage_mb: 0,
        network_kb_per_minute: 0,
    }));
    let visual = VisualObservationPermissionProvider::validation_decision_for_state(
        VisualObservationPermissionState::Granted,
    );
    let request = timeout_request();
    let invalidated_sequence = request.invalidated_binding_sequence();

    let failure = AxBoundedVisualEscalationAuthority::new()
        .authorize_visual_observation(
            request,
            visual,
            &governor,
            "m10-session",
            "m10-resource-denied",
        )
        .expect_err("resource governor denial must block timeout-driven visual escalation");

    assert!(matches!(
        failure.error(),
        AxVisualEscalationAuthorizationError::ResourceDenied(_)
    ));
    let recovered = failure.into_request();
    assert_eq!(recovered.invalidated_binding_sequence(), invalidated_sequence);
    assert_eq!(recovered.target_identity(), &identity());
}

#[test]
fn visual_permission_denial_preserves_timeout_request_for_semantic_reconciliation() {
    let governor = RuntimeResourceGovernor::default();
    let visual = VisualObservationPermissionProvider::validation_decision_for_state(
        VisualObservationPermissionState::Denied,
    );
    let request = timeout_request();
    let invalidated_sequence = request.invalidated_binding_sequence();

    let failure = AxBoundedVisualEscalationAuthority::new()
        .authorize_visual_observation(
            request,
            visual,
            &governor,
            "m10-session",
            "m10-permission-denied",
        )
        .expect_err("visual permission denial must fail closed");

    assert!(matches!(
        failure.error(),
        AxVisualEscalationAuthorizationError::VisualPermission(
            VisualObservationPermissionError::PermissionRequired
        )
    ));
    let recovered = failure.into_request();
    assert_eq!(recovered.invalidated_binding_sequence(), invalidated_sequence);
    assert_eq!(recovered.target_identity(), &identity());
}
