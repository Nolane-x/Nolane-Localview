const CARGO_TOML: &str = include_str!("../Cargo.toml");
const VISUAL_CAPTURE_SOURCE: &str = include_str!("../src/visual_capture.rs");

fn function_slice<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source
        .find(start)
        .unwrap_or_else(|| panic!("missing function marker: {start}"));
    let tail = &source[start_index..];
    let end_index = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing function end marker: {end}"));
    &tail[..end_index]
}

fn ordered_positions(source: &str, markers: &[&str]) -> Vec<usize> {
    let mut positions = Vec::with_capacity(markers.len());
    let mut cursor = 0usize;
    for marker in markers {
        let relative = source[cursor..]
            .find(marker)
            .unwrap_or_else(|| panic!("missing authority marker: {marker}"));
        let absolute = cursor + relative;
        positions.push(absolute);
        cursor = absolute + marker.len();
    }
    positions
}

#[test]
fn desktop_owns_the_retained_resource_ledger_locally() {
    assert!(
        CARGO_TOML.contains(
            "localview-resource-governor = { path = \"../../../crates/resource-governor\" }"
        ),
        "desktop must depend directly on the owner-local resource governor"
    );
    assert!(VISUAL_CAPTURE_SOURCE.contains("RetainedResourceLedger"));
    assert!(VISUAL_CAPTURE_SOURCE.contains("RetainedResourceKind::CaptureStorage"));
    assert!(VISUAL_CAPTURE_SOURCE.contains("RetainedResourceKind::Cache"));
    assert!(VISUAL_CAPTURE_SOURCE.contains("projected_used_bytes_after_put"));
    assert!(VISUAL_CAPTURE_SOURCE.contains("projected_used_bytes_after_insert"));
    assert!(VISUAL_CAPTURE_SOURCE.contains("used_bytes()"));
    assert!(
        !VISUAL_CAPTURE_SOURCE.contains("/v1/runtime/resources/retained"),
        "retained-resource authority must not be caller-writable over HTTP"
    );
}

#[test]
fn artifact_mutation_is_reconcile_project_admit_mutate_reconcile() {
    let function = function_slice(
        VISUAL_CAPTURE_SOURCE,
        "async fn persist_and_register(",
        "use localview_capture::{",
    );
    let positions = ordered_positions(
        function,
        &[
            "retained_resources.synchronize(",
            "projected_used_bytes_after_put",
            "retained_resources.admit_projected(",
            ".put(\"visual/png\", &png)",
            "retained_resources.synchronize(",
        ],
    );
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn baseline_mutation_is_reconcile_project_admit_mutate_reconcile() {
    let function = function_slice(
        VISUAL_CAPTURE_SOURCE,
        "async fn commit_changed_baseline(",
        "fn changed_baseline_context(",
    );
    let positions = ordered_positions(
        function,
        &[
            "retained_resources.synchronize(",
            "projected_used_bytes_after_insert",
            "retained_resources.admit_projected(",
            ".insert(session_id, context, image)",
            "retained_resources.synchronize(",
        ],
    );
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
}
