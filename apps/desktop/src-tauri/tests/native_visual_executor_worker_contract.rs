use std::{fs, path::PathBuf};

fn source(path: &str) -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(manifest.join(path))
        .unwrap_or_else(|error| panic!("required native executor source {path} is missing: {error}"))
}

#[test]
fn desktop_spawns_a_private_native_executor_worker() {
    let lib = source("src/lib.rs");

    assert!(lib.contains("mod native_executor_worker;"));
    assert!(lib.contains("native_executor_worker::spawn(app.handle().clone())"));
    assert_eq!(
        lib.matches("native_executor_worker::spawn(").count(),
        1,
        "desktop must own exactly one native executor polling loop"
    );
}

#[test]
fn worker_polls_and_completes_existing_daemon_requests_without_a_creation_api() {
    let worker = source("src/native_executor_worker.rs");

    assert!(worker.contains("/native-executor\""));
    assert!(worker.contains("/native-executor/results"));
    assert!(worker.contains(".get("));
    assert!(worker.contains(".post("));
    assert!(worker.contains("visual_capture::execute_native_visual_packet"));
    assert!(!worker.contains("capture_webview"));
    assert!(!worker.contains("capture_visual_packet_authorized"));
    assert!(!worker.contains("#[tauri::command]"));
    assert!(
        !worker.contains(".post(format!(\n            \"http://127.0.0.1:45454/v1/sessions/{session_id}/native-executor\""),
        "worker must never gain a native request creation POST"
    );
}

#[test]
fn only_the_internal_adapter_can_forward_planner_owned_visual_authority() {
    let packet = source("src/visual_packet_impl.rs");
    let start = packet
        .find("pub(crate) async fn execute_native_visual_packet(")
        .expect("internal native visual adapter must exist");
    let adapter = &packet[start..];

    assert!(adapter.contains("NativeExecutorRequest"));
    assert!(adapter.contains("NativeExecutorResult"));
    assert!(adapter.contains("capture_visual_packet_authorized("));
    assert!(adapter.contains("budget_escalation_reason"));
    assert!(!adapter.contains("#[tauri::command]"));
}
