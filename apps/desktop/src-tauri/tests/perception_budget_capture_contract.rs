#[test]
fn visual_packet_accepts_the_full_spec_budget_and_explicit_escalation_reason() {
    let module = include_str!("../src/visual_packet_impl.rs");

    assert!(module.contains("budget: PerceptionBudgetContract"));
    assert!(module.contains("budget_escalation_reason: Option<BudgetEscalationReason>"));
    assert!(module.contains("budget.visual_packet_budget(DetailLevel::Normal)"));
}

#[test]
fn positive_image_path_evaluates_budget_before_any_selected_evidence_is_persisted() {
    let module = include_str!("../src/visual_packet_impl.rs");
    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = module[start..]
        .find("\nfn changed_plan_ratio")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    let select = command
        .find("select_visual_packet(")
        .expect("packet selection must happen before budget admission");
    let evaluate = command
        .find("evaluate_perception_budget(")
        .expect("full perception budget must be evaluated");
    let persist = command
        .find("persist_visual_packet_selection(")
        .expect("selected evidence must persist only after budget admission");
    let commit = command
        .find("commit_changed_baseline(")
        .expect("baseline must advance after persistence");

    assert!(select < evaluate);
    assert!(evaluate < persist);
    assert!(persist < commit);
}

#[test]
fn current_native_visual_packet_records_zero_chromium_spawns_and_measured_usage() {
    let module = include_str!("../src/visual_packet_impl.rs");

    assert!(module.contains("let started_at = Instant::now();"));
    assert!(module.contains("latency_ms: elapsed_ms(started_at)"));
    assert!(module.contains("text_tokens: visual_packet_text_tokens(&packet)"));
    assert!(module.contains("image_regions: selection.selected.len()"));
    assert!(module.contains("chromium_spawns: 0"));
}

#[test]
fn budget_decision_and_escalation_reason_are_exposed_in_the_receipt() {
    let module = include_str!("../src/visual_packet_impl.rs");

    assert!(module.contains("pub budget_decision: PerceptionBudgetDecision"));
    assert!(module.contains("budget_decision,"));
}

#[test]
fn zero_image_budget_still_runs_the_budget_contract_without_native_capture() {
    let module = include_str!("../src/visual_packet_impl.rs");
    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = module[start..]
        .find("let capture_gate = session_capture_gate")
        .map(|offset| start + offset)
        .expect("positive image path must acquire the session gate");
    let prefix = &module[start..end];

    assert!(prefix.contains("if budget.image_regions == 0"));
    assert!(prefix.contains("evaluate_perception_budget("));
    assert!(!prefix.contains("capture_redacted_viewport_after_gate("));
}
