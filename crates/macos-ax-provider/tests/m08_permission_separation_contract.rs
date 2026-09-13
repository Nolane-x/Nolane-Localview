use localview_macos_ax_provider::{
    AxPermissionProvider, VisualObservationPermissionProvider,
    VisualObservationPermissionState,
};

#[test]
fn visual_observation_and_accessibility_are_separate_authority_domains() {
    let ax = AxPermissionProvider::new().semantic_control_decision(false);
    let visual = VisualObservationPermissionProvider::new().observation_decision();

    assert!(ax.revision().check_sequence() > 0);
    assert!(visual.revision().check_sequence() > 0);

    match visual.revision().state() {
        VisualObservationPermissionState::Granted => assert!(visual.permit().is_some()),
        VisualObservationPermissionState::Denied | VisualObservationPermissionState::Unknown => {
            assert!(visual.permit().is_none())
        }
    }

    if let Some(ax_permit) = ax.permit() {
        assert_eq!(ax_permit.permission_check_sequence(), ax.revision().check_sequence());
    }

    if let Some(visual_permit) = visual.permit() {
        assert_eq!(
            visual_permit.permission_check_sequence(),
            visual.revision().check_sequence()
        );
    }
}
