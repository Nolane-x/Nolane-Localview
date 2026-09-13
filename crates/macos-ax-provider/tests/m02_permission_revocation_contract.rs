use localview_macos_ax_provider::{AxPermissionProvider, AxPermissionState};

#[test]
fn semantic_dispatch_rechecks_live_permission_after_prior_admission() {
    let provider = AxPermissionProvider::new();
    let admission = provider.semantic_control_decision(false);

    let Some(permit) = admission.permit() else {
        // This contract is portable across macOS machines whose current TCC
        // state may already be denied. The real M02 seed establishes the
        // trusted -> revoked topology explicitly; this focused contract proves
        // that a previously admitted permit cannot be dispatched without a
        // second provider-owned OS observation.
        assert_ne!(admission.revision().state(), AxPermissionState::Trusted);
        return;
    };

    let dispatch = provider.semantic_dispatch_decision(permit);

    assert!(
        dispatch.revision().check_sequence() > permit.permission_check_sequence(),
        "dispatch must be fenced by a newer live AX permission observation"
    );
}
