fn packet_module() -> &'static str {
    include_str!("../src/visual_packet_impl.rs")
}

#[test]
fn visual_packet_command_is_registered_and_uses_the_shared_native_capture_authority() {
    let lib = include_str!("../src/lib.rs");
    let shared = include_str!("../src/visual_capture.rs");
    let module = packet_module();

    assert!(lib.contains("visual_capture::capture_visual_packet"));
    assert!(shared.contains("include!(\"visual_packet_impl.rs\");"));
    assert!(module.contains("pub async fn capture_visual_packet("));

    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = module[start..]
        .find("\nfn changed_plan_ratio")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    assert!(command.contains("session_capture_gate(&state, session_id).await?"));
    assert_eq!(
        command.matches("capture_redacted_viewport_after_gate(").count(),
        1,
        "visual packet selection must acquire native pixels once"
    );
    assert!(!command.contains("capture_progressive_target("));
    assert!(!command.contains("capture_changed_regions("));
    assert!(!command.contains("capture_managed_surface("));
}

#[test]
fn visual_packet_combines_fresh_semantic_targets_and_changed_regions_before_selection() {
    let module = packet_module();
    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = module[start..]
        .find("\nfn changed_plan_ratio")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    let fresh = command
        .find("fresh_semantic_snapshot(session_id).await?")
        .expect("reference-scoped packet must use a fresh snapshot");
    let resolve = command
        .find("resolve_progressive_targets(&snapshot, reference)")
        .expect("fresh snapshot must feed the semantic target resolver");
    let acquire = command
        .find("capture_redacted_viewport_after_gate(")
        .expect("packet must use the audited private capture transaction");
    let diff = command
        .find("plan_changed_css_regions(")
        .expect("packet must consult the changed-region planner");
    let select = command
        .find("select_visual_packet(")
        .expect("packet must use the deterministic token-budget policy");

    assert!(fresh < resolve);
    assert!(resolve < acquire);
    assert!(acquire < diff);
    assert!(diff < select);
}

#[test]
fn visual_packet_never_exposes_unredacted_pixels_to_selection_or_persistence() {
    let shared_module = include_str!("../src/visual_capture.rs");
    let packet = packet_module();

    let shared_start = shared_module
        .find("async fn capture_redacted_viewport_after_gate(")
        .expect("shared redacted viewport helper must exist");
    let shared_end = shared_module[shared_start..]
        .find("\nasync fn ")
        .map(|offset| shared_start + offset)
        .unwrap_or(shared_module.len());
    let shared = &shared_module[shared_start..shared_end];
    let restore = shared
        .find("restore_visual_state(session_id, &freeze.token).await")
        .expect("exact visual state must be restored");
    let redact = shared
        .find("redact_private_pixels(frame, &freeze)?")
        .expect("private pixels must be redacted in memory");
    assert!(restore < redact);

    let start = packet
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = packet[start..]
        .find("\nfn changed_plan_ratio")
        .map(|offset| start + offset)
        .unwrap_or(packet.len());
    let command = &packet[start..end];
    let decode = command
        .find("decode_png_rgba(&frame.png)")
        .expect("selection must decode only the already-redacted frame");
    let select = command
        .find("select_visual_packet(")
        .expect("selection must occur after private redaction");
    let persist = command
        .find("persist_visual_packet_selection(")
        .expect("only selected processed evidence may persist");
    assert!(decode < select);
    assert!(select < persist);
}

#[test]
fn zero_image_budget_short_circuits_before_any_native_acquisition() {
    let module = packet_module();
    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = module[start..]
        .find("\nfn changed_plan_ratio")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    let budget_guard = command
        .find("if budget.image_regions == 0")
        .expect("zero image budget must be explicit");
    let acquire = command
        .find("capture_redacted_viewport_after_gate(")
        .expect("non-zero budget path must capture once");
    assert!(budget_guard < acquire);
    assert!(command.contains("VisualPacketSelectionMode::MetadataOnly"));
}

#[test]
fn visual_packet_baseline_advances_only_after_selected_evidence_succeeds() {
    let module = packet_module();
    let start = module
        .find("pub async fn capture_visual_packet(")
        .expect("visual packet command must exist");
    let end = module[start..]
        .find("\nfn changed_plan_ratio")
        .map(|offset| start + offset)
        .unwrap_or(module.len());
    let command = &module[start..end];

    let persist = command
        .find("persist_visual_packet_selection(")
        .expect("selected packet evidence must be persisted");
    let commit = command
        .find("commit_changed_baseline(")
        .expect("successful packet must advance the private baseline");
    assert!(persist < commit);
}

#[test]
fn viewport_fallback_costs_the_true_viewport_and_does_not_clone_full_rgba() {
    let module = packet_module();
    assert!(module.contains("width: viewport.0 as f64"));
    assert!(module.contains("height: viewport.1 as f64"));
    assert!(!module.contains("RequestedCaptureTarget::Viewport => image.clone()"));
}
