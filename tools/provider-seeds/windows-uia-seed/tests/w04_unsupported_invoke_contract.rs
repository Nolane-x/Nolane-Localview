use localview_windows_uia_seed::{SeedCommand, SeedState};
use uuid::Uuid;

fn fixture() -> SeedState {
    SeedState::new(
        Uuid::from_u128(1),
        Uuid::from_u128(2),
        100,
        200,
        Uuid::from_u128(3),
        "initial".to_owned(),
    )
}

#[test]
fn w04_ground_truth_distinguishes_supported_from_unsupported_invoke() {
    let mut state = fixture();
    assert!(
        state.ground_truth().expected_invoke_support,
        "the existing BUTTON seed begins as a real Invoke-capable control"
    );

    let truth = state
        .record_unsupported_invoke_control(201, Uuid::from_u128(4))
        .expect("W04 replacement should be accepted while the seed is live");

    assert_eq!(truth.control_handle, 201);
    assert_eq!(truth.control_incarnation, Uuid::from_u128(4));
    assert!(!truth.expected_invoke_support);
    assert_eq!(truth.recreation_generation, 2);
    assert_eq!(truth.logical_sequence, 2);
}

#[test]
fn w04_control_protocol_has_a_stable_unsupported_invoke_command() {
    assert_eq!(
        serde_json::from_str::<SeedCommand>(
            r#"{"command":"present_unsupported_invoke_control"}"#,
        )
        .unwrap(),
        SeedCommand::PresentUnsupportedInvokeControl,
    );
}
