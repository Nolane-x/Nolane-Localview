use localview_macos_ax_provider::{
    AxEventAssurance, AxNotificationRegistration, AxNotificationRegistrationOutcome,
    AxNotificationRequest, AxObserverReliabilityProfile,
};

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_NOTIFICATION_UNSUPPORTED: i32 = -25207;
const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;

fn request(notification: &str, semantic_dimension: &str) -> AxNotificationRequest {
    AxNotificationRequest::new(notification, semantic_dimension)
}

#[test]
fn all_requested_dimensions_registered_may_be_event_complete() {
    let profile = AxObserverReliabilityProfile::from_registrations(vec![
        AxNotificationRegistration::from_ax_error(
            request("AXTitleChanged", "window.title"),
            AX_ERROR_SUCCESS,
        ),
        AxNotificationRegistration::from_ax_error(
            request("AXFocusedUIElementChanged", "focus.current_element"),
            AX_ERROR_SUCCESS,
        ),
    ]);

    assert_eq!(
        profile.assurance(),
        AxEventAssurance::CompleteForRequestedDimensions
    );
    assert!(profile.unsupported_notifications().is_empty());
    assert!(profile.incomplete_semantic_dimensions().is_empty());
}

#[test]
fn unsupported_notification_is_explicit_and_forces_incomplete_event_assurance() {
    let supported = AxNotificationRegistration::from_ax_error(
        request("AXTitleChanged", "window.title"),
        AX_ERROR_SUCCESS,
    );
    let unsupported = AxNotificationRegistration::from_ax_error(
        request("AXSelectedRowsChanged", "table.selection"),
        AX_ERROR_NOTIFICATION_UNSUPPORTED,
    );

    assert_eq!(
        unsupported.outcome(),
        AxNotificationRegistrationOutcome::Unsupported
    );

    let profile = AxObserverReliabilityProfile::from_registrations(vec![supported, unsupported]);
    assert_eq!(profile.assurance(), AxEventAssurance::Incomplete);
    assert_eq!(
        profile.unsupported_notifications(),
        vec!["AXSelectedRowsChanged"]
    );
    assert_eq!(
        profile.incomplete_semantic_dimensions(),
        vec!["table.selection"]
    );
}

#[test]
fn inconclusive_registration_error_cannot_be_laundered_into_complete_coverage() {
    let failed = AxNotificationRegistration::from_ax_error(
        request("AXValueChanged", "control.value"),
        AX_ERROR_CANNOT_COMPLETE,
    );

    assert_eq!(
        failed.outcome(),
        AxNotificationRegistrationOutcome::Failed {
            ax_error: AX_ERROR_CANNOT_COMPLETE,
        }
    );

    let profile = AxObserverReliabilityProfile::from_registrations(vec![failed]);
    assert_eq!(profile.assurance(), AxEventAssurance::Incomplete);
    assert_eq!(
        profile.incomplete_semantic_dimensions(),
        vec!["control.value"]
    );
}

#[test]
fn unsupported_event_dimension_does_not_claim_direct_read_is_unavailable() {
    let profile = AxObserverReliabilityProfile::from_registrations(vec![
        AxNotificationRegistration::from_ax_error(
            request("AXSelectedRowsChanged", "table.selection"),
            AX_ERROR_NOTIFICATION_UNSUPPORTED,
        ),
    ]);

    assert_eq!(profile.assurance(), AxEventAssurance::Incomplete);
    assert!(profile.requires_snapshot_reconciliation());
    assert_eq!(
        profile.incomplete_semantic_dimensions(),
        vec!["table.selection"]
    );
}

#[test]
fn empty_requested_set_never_mints_completeness() {
    let profile = AxObserverReliabilityProfile::from_registrations(Vec::new());

    assert_eq!(profile.assurance(), AxEventAssurance::Incomplete);
    assert!(profile.requires_snapshot_reconciliation());
}
