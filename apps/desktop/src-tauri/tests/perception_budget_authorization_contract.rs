fn packet_module() -> &'static str {
    include_str!("../src/visual_packet_impl.rs")
}

#[test]
fn public_visual_packet_command_cannot_supply_an_escalation_reason() {
    let module = packet_module();
    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("public visual packet command must exist");
    let end = module[start..]
        .find("\nasync fn capture_visual_packet_authorized(")
        .map(|offset| start + offset)
        .expect("public command must delegate to a private authorized path");
    let public_command = &module[start..end];

    assert!(public_command.contains("budget: PerceptionBudgetContract"));
    assert!(!public_command.contains("budget_escalation_reason:"));
    assert!(public_command.contains("capture_visual_packet_authorized("));
    assert!(public_command.contains("None,"));
}

#[test]
fn escalation_reason_exists_only_on_the_internal_authorized_capture_path() {
    let module = packet_module();
    let start = module
        .find("async fn capture_visual_packet_authorized(")
        .expect("private authorized capture path must exist");
    let end = module[start..]
        .find("\nfn elapsed_ms")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let authorized = &module[start..end];

    assert!(authorized.contains("budget_escalation_reason: Option<BudgetEscalationReason>"));
    assert!(authorized.contains("evaluate_perception_budget("));
    assert!(!authorized.contains("#[tauri::command]"));
}

#[test]
fn command_registry_exposes_only_the_non_escalating_public_wrapper() {
    let lib = include_str!("../src/lib.rs");
    assert!(lib.contains("visual_capture::capture_visual_packet"));
    assert!(!lib.contains("capture_visual_packet_authorized"));
}
