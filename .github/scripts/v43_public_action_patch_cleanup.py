from pathlib import Path
import subprocess

baseline = subprocess.check_output(
    ["git", "show", "origin/main:crates/control/src/runtime.rs"], text=True
)
needle = '''    if request.action.is_internal_capture_action() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "internal_capture_action_not_public"
            })),
        )
            .into_response();
    }
    let action = state
'''
replacement = '''    if request.action.is_internal_capture_action() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "internal_capture_action_not_public"
            })),
        )
            .into_response();
    }
    if !matches!(&request.action, BridgeActionKind::Snapshot) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "canonical_consequential_action_authority_required",
                "message": "UI-changing actions must use the canonical V4.3 consequential authority path"
            })),
        )
            .into_response();
    }
    let action = state
'''
if needle not in baseline:
    raise SystemExit("main baseline insertion point not found")
Path("crates/control/src/runtime.rs").write_text(baseline.replace(needle, replacement, 1))
