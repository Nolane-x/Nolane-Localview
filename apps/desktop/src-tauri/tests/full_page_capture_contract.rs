#[test]
fn desktop_full_page_transaction_is_single_gate_restore_before_persistence() {
    let source = include_str!("../src/visual_capture.rs");

    assert!(source.contains("pub struct FullPageCaptureReceipt"));
    assert!(source.contains("pub async fn capture_full_page("));
    assert!(source.contains("const FULL_PAGE_VISUAL_FREEZE_LEASE_MS: u64 = 30_000;"));
    assert!(source.contains("const FULL_PAGE_TRANSACTION_TIMEOUT_MS: u64 = 30_000;"));
    assert!(source.contains("plan_full_page("));
    assert!(source.contains("project_full_page_output("));
    assert!(source.contains("stitch_full_page_tile("));

    let command_start = source
        .find("pub async fn capture_full_page(")
        .expect("full-page command must exist");
    let after = &source[command_start..];
    let gate = after
        .find("session_capture_gate")
        .expect("full-page command must acquire the existing session gate");
    let transaction = after
        .find("full_page_capture_after_gate")
        .expect("full-page command must enter one guarded transaction");
    assert!(gate < transaction);

    let work_start = source
        .find("async fn capture_full_page_tiles(")
        .expect("bounded tile worker must exist");
    let work_end = source[work_start..]
        .find("async fn cleanup_full_page_state")
        .map(|offset| work_start + offset)
        .expect("cleanup must follow tile work");
    let work = &source[work_start..work_end];

    let scroll = work
        .find("capture_scroll_to")
        .expect("every tile must use private absolute scroll");
    let settle = work[scroll..]
        .find("wait_for_capture_settle")
        .map(|offset| scroll + offset)
        .expect("every tile must settle after scroll");
    let probe = work[settle..]
        .find("capture_tile_probe")
        .map(|offset| settle + offset)
        .expect("every tile must refresh private mask geometry");
    let native = work[probe..]
        .find("capture_managed_surface")
        .map(|offset| probe + offset)
        .expect("native viewport capture must follow the fresh probe");
    let redact = work[native..]
        .find("redact_png_css_rects")
        .map(|offset| native + offset)
        .expect("tile redaction must happen before decode/stitch");
    let decode = work[redact..]
        .find("decode_png_rgba")
        .map(|offset| redact + offset)
        .expect("redacted tile must be decoded");
    let stitch = work[decode..]
        .find("stitch_full_page_tile")
        .map(|offset| decode + offset)
        .expect("decoded tile must be stitched");
    assert!(scroll < settle && settle < probe && probe < native);
    assert!(native < redact && redact < decode && decode < stitch);
    assert!(work.contains("visible_fixed_or_sticky"));
    assert!(work.contains("full_page_fixed_or_sticky_unsupported"));
    assert!(work.contains("validate_capture_scroll_receipt"));
    assert!(work.contains("validate_capture_tile_probe"));
    assert!(work.contains("full_page_route_drift"));
    assert!(work.contains("full_page_native_geometry_drift"));
    assert!(source.contains("full_page_document_geometry_drift"));
    assert!(source.contains("full_page_viewport_geometry_drift"));
    assert!(!work.contains("Vec<CapturedFrame>"));
    assert!(!work.contains("Vec<RgbaImage>"));

    let transaction_start = source
        .find("async fn full_page_capture_after_gate(")
        .expect("guarded coordinator must exist");
    let transaction_end = source[transaction_start..]
        .find("async fn capture_full_page_tiles(")
        .map(|offset| transaction_start + offset)
        .expect("tile worker must follow coordinator");
    let transaction = &source[transaction_start..transaction_end];
    let initial_settle = transaction
        .find("wait_for_capture_settle")
        .expect("transaction must settle before freeze");
    let freeze = transaction
        .find("freeze_full_page_visual_state")
        .expect("transaction must use dedicated full-page freeze");
    let tile_work = transaction
        .find("capture_full_page_tiles")
        .expect("transaction must execute bounded tile work");
    let cleanup = transaction
        .find("cleanup_full_page_state")
        .expect("transaction must always enter cleanup after tile work");
    let encode = transaction
        .find("encode_png_rgba")
        .expect("final encoding must occur after cleanup");
    let persist = transaction
        .find("persist_full_page_and_register")
        .expect("artifact/evidence persistence must be last");
    assert!(initial_settle < freeze && freeze < tile_work && tile_work < cleanup);
    assert!(cleanup < encode && encode < persist);
    assert!(transaction.contains("full_page_transaction_timeout"));
    assert!(transaction.contains("FULL_PAGE_CLEANUP_RESERVE_MS"));
    assert!(transaction.contains("work_deadline"));
    assert!(transaction.contains("cleanup_full_page_state(session_id, &viewport, &freeze, deadline)"));
}

#[test]
fn full_page_cleanup_restores_scroll_before_visual_state() {
    let source = include_str!("../src/visual_capture.rs");
    let start = source
        .find("async fn cleanup_full_page_state")
        .expect("full-page cleanup must exist");
    let end = source[start..]
        .find("async fn persist_full_page_and_register")
        .map(|offset| start + offset)
        .expect("persistence helper must follow cleanup");
    let cleanup = &source[start..end];

    let scroll = cleanup
        .find("capture_scroll_to")
        .expect("cleanup must explicitly restore original scroll");
    let restore = cleanup
        .find("restore_visual_state")
        .expect("cleanup must restore visuals even after scroll restoration attempt");
    assert!(scroll < restore);
    assert!(cleanup.contains("let original_scroll_y = freeze.scroll_y;"));
    assert!(cleanup.contains("full_page_scroll_restore_failed"));
    assert!(cleanup.contains("full_page_visual_restore_failed"));
}

#[test]
fn full_page_persistence_uses_one_final_artifact_and_dedicated_evidence() {
    let source = include_str!("../src/visual_capture.rs");
    let start = source
        .find("async fn persist_full_page_and_register")
        .expect("dedicated final persistence helper must exist");
    let end = source[start..]
        .find("async fn freeze_full_page_visual_state")
        .map(|offset| start + offset)
        .expect("full-page freeze helper must follow final persistence");
    let persist = &source[start..end];

    assert!(persist.contains("artifacts.put(\"visual/png\", &png)"));
    assert!(persist.contains("/evidence/visual-full-page"));
    assert!(persist.contains("FullPageVisualEvidenceRequest"));
    assert!(!persist.contains("capture_managed_surface"));
    assert!(!persist.contains("capture_tile_probe"));
    assert!(!persist.contains("freeze_full_page_visual_state"));
}

#[test]
fn tauri_registers_explicit_full_page_command_without_platform_full_page_adapter() {
    let desktop = include_str!("../src/lib.rs");
    assert!(desktop.contains("visual_capture::capture_full_page"));

    let native = include_str!("../../../../crates/native-capture/src/lib.rs");
    assert!(native.contains("if request.target != CaptureTarget::Viewport"));
    assert!(!native.contains("FullPage"));
    assert!(!native.contains("full_page"));
}


#[test]
fn final_encode_and_persistence_remain_under_absolute_transaction_deadline() {
    let source = include_str!("../src/visual_capture.rs");
    let start = source
        .find("async fn full_page_capture_after_gate(")
        .expect("guarded full-page coordinator must exist");
    let end = source[start..]
        .find("async fn capture_full_page_tiles(")
        .map(|offset| start + offset)
        .expect("tile worker must follow coordinator");
    let transaction = &source[start..end];

    let cleanup = transaction
        .find("cleanup_full_page_state")
        .expect("cleanup must complete before final encoding");
    let encode = transaction
        .find("encode_png_rgba")
        .expect("final PNG encoding must exist");
    assert!(cleanup < encode);

    let after_encode = &transaction[encode..];
    let post_encode_deadline = after_encode
        .find("tokio::time::Instant::now() >= deadline")
        .expect("deadline must be rechecked after synchronous final encoding");
    let bounded_persistence = after_encode
        .find("tokio::time::timeout_at(")
        .expect("artifact/evidence persistence must remain inside the absolute deadline");
    let persistence_call = after_encode
        .find("persist_full_page_and_register(")
        .expect("dedicated final persistence call must exist");

    assert!(post_encode_deadline < bounded_persistence);
    assert!(bounded_persistence < persistence_call);
    assert!(after_encode[bounded_persistence..persistence_call].contains("deadline"));
    assert!(after_encode.contains("full_page_transaction_timeout"));
}
