use localview_macos_ax_provider::{
    AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
};

#[test]
fn permission_revisions_are_provider_owned_and_monotonic() {
    let provider = AxPermissionProvider::new();
    let first = provider.current_permission_revision(false);
    let second = provider.current_permission_revision(true);

    assert!(!first.prompt_requested());
    assert!(second.prompt_requested());
    assert!(
        second.check_sequence() > first.check_sequence(),
        "each OS permission observation must advance the revision sequence"
    );
}

#[test]
fn semantic_control_decision_is_bound_to_its_own_fresh_permission_revision() {
    let provider = AxPermissionProvider::new();
    let decision = provider.semantic_control_decision(false);
    let revision = decision.revision();

    assert!(!revision.prompt_requested());
    assert!(revision.check_sequence() > 0);

    match revision.state() {
        AxPermissionState::Trusted => {
            let permit = decision
                .permit()
                .expect("trusted OS observation must be the only permit-producing state");
            assert_eq!(
                permit.permission_check_sequence(),
                revision.check_sequence(),
                "permit must bind the exact OS-backed permission revision"
            );
            assert_eq!(
                decision.outcome(),
                AxSemanticControlOutcome::Authorized(permit)
            );
        }
        AxPermissionState::Untrusted => {
            assert_eq!(decision.permit(), None);
            assert_eq!(decision.denial(), Some(AxPermissionError::PermissionRequired));
        }
        AxPermissionState::Unknown => {
            assert_eq!(decision.permit(), None);
            assert_eq!(decision.denial(), Some(AxPermissionError::PermissionUnknown));
        }
    }
}
