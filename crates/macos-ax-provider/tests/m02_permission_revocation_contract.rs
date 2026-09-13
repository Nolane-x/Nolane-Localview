use localview_macos_ax_provider::{
    AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
};

#[test]
fn semantic_dispatch_rechecks_permission_through_a_fresh_process_probe() {
    let probe = env!("CARGO_BIN_EXE_localview-ax-permission-probe");
    let provider = AxPermissionProvider::with_dispatch_probe_executable(probe);
    let admission = provider.semantic_control_decision(false);

    let Some(permit) = admission.permit() else {
        // Portable across machines whose current TCC state starts denied. The
        // real-provider seed separately establishes trusted -> revoked.
        assert_ne!(admission.revision().state(), AxPermissionState::Trusted);
        return;
    };

    let dispatch = provider.semantic_dispatch_decision(permit);

    assert!(
        dispatch.revision().check_sequence() > permit.permission_check_sequence(),
        "dispatch must be fenced by a newer provider-owned permission revision"
    );
    assert_eq!(
        dispatch.revision().state(),
        AxPermissionState::Trusted,
        "a fresh probe built from the same provider must observe the still-trusted precondition"
    );
    assert!(matches!(
        dispatch.outcome(),
        AxSemanticControlOutcome::Authorized(_)
    ));
}

#[test]
fn unavailable_fresh_process_probe_fails_closed_instead_of_reusing_prior_authority() {
    let admitted = AxPermissionProvider::new().semantic_control_decision(false);
    let Some(permit) = admitted.permit() else {
        return;
    };

    let provider = AxPermissionProvider::with_dispatch_probe_executable(
        "/definitely/not/a/localview/permission/probe",
    );
    let dispatch = provider.semantic_dispatch_decision(permit);

    assert_eq!(dispatch.revision().state(), AxPermissionState::Unknown);
    assert_eq!(
        dispatch.outcome(),
        AxSemanticControlOutcome::Denied(AxPermissionError::PermissionUnknown)
    );
    assert_eq!(dispatch.permit(), None);
}
