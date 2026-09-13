use localview_macos_ax_provider::{
    AxPermissionError, AxPermissionProvider, AxPermissionRevision, AxPermissionState,
};

#[test]
fn untrusted_accessibility_permission_cannot_mint_semantic_control_authority() {
    let provider = AxPermissionProvider::new();
    let revision = AxPermissionRevision::observed(AxPermissionState::Untrusted, 1, false);

    let error = provider
        .authorize_semantic_control(&revision)
        .expect_err("untrusted Accessibility permission must not mint semantic-control authority");

    assert_eq!(error, AxPermissionError::PermissionRequired);
}

#[test]
fn prompt_requested_is_not_permission_granted() {
    let provider = AxPermissionProvider::new();
    let revision = AxPermissionRevision::observed(AxPermissionState::Untrusted, 2, true);

    assert!(revision.prompt_requested());
    assert_eq!(revision.state(), AxPermissionState::Untrusted);
    assert_eq!(
        provider.authorize_semantic_control(&revision),
        Err(AxPermissionError::PermissionRequired),
        "requesting the OS prompt must never be projected into trusted authority"
    );
}

#[test]
fn unknown_permission_is_distinct_from_explicit_denial() {
    let provider = AxPermissionProvider::new();
    let revision = AxPermissionRevision::observed(AxPermissionState::Unknown, 3, false);

    assert_eq!(
        provider.authorize_semantic_control(&revision),
        Err(AxPermissionError::PermissionUnknown)
    );
}
