use localview_windows_uia_seed::{
    SeedCommand, SeedState, SeedStateError,
};
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
fn initial_ground_truth_starts_at_generation_one() {
    let state = fixture();
    let truth = state.ground_truth();

    assert_eq!(truth.recreation_generation, 1);
    assert_eq!(truth.logical_sequence, 1);
    assert_eq!(truth.window_handle, 100);
    assert_eq!(truth.control_handle, 200);
    assert_eq!(truth.logical_name, "initial");
}

#[test]
fn burst_name_changes_retains_last_name_and_advances_sequence_per_mutation() {
    let mut state = fixture();
    let before = state.ground_truth().logical_sequence;
    let truth = state
        .burst_name_changes(&["A".into(), "B".into(), "C".into()])
        .expect("burst should be accepted");

    assert_eq!(truth.logical_name, "C");
    assert_eq!(truth.logical_sequence, before + 3);
    assert_eq!(truth.recreation_generation, 1);
}

#[test]
fn recreate_changes_control_incarnation_and_generation_without_replacing_window() {
    let mut state = fixture();
    let previous = state.ground_truth();
    let next_incarnation = Uuid::from_u128(4);

    let truth = state
        .record_recreated_control(201, next_incarnation)
        .expect("recreate should be accepted");

    assert_eq!(truth.window_handle, previous.window_handle);
    assert_eq!(truth.control_handle, 201);
    assert_ne!(truth.control_incarnation, previous.control_incarnation);
    assert_eq!(truth.control_incarnation, next_incarnation);
    assert_eq!(truth.recreation_generation, previous.recreation_generation + 1);
    assert_eq!(truth.logical_sequence, previous.logical_sequence + 1);
}

#[test]
fn shutdown_is_terminal() {
    let mut state = fixture();
    let shutdown_truth = state.shutdown().expect("first shutdown should succeed");
    assert!(shutdown_truth.terminal);

    assert_eq!(
        state.burst_name_changes(&["after".into()]),
        Err(SeedStateError::Terminal),
    );
    assert_eq!(
        state.record_recreated_control(202, Uuid::from_u128(5)),
        Err(SeedStateError::Terminal),
    );
}

#[test]
fn json_line_commands_use_stable_protocol_names() {
    assert_eq!(
        serde_json::from_str::<SeedCommand>(r#"{"command":"get_ground_truth"}"#).unwrap(),
        SeedCommand::GetGroundTruth,
    );
    assert_eq!(
        serde_json::from_str::<SeedCommand>(
            r#"{"command":"burst_name_changes","names":["A","B","C"]}"#,
        )
        .unwrap(),
        SeedCommand::BurstNameChanges {
            names: vec!["A".into(), "B".into(), "C".into()],
        },
    );
    assert_eq!(
        serde_json::from_str::<SeedCommand>(r#"{"command":"recreate_control"}"#).unwrap(),
        SeedCommand::RecreateControl,
    );
    assert_eq!(
        serde_json::from_str::<SeedCommand>(r#"{"command":"shutdown"}"#).unwrap(),
        SeedCommand::Shutdown,
    );
}
