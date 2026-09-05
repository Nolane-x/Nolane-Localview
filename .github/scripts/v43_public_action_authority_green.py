from pathlib import Path

path = Path("crates/control/src/runtime.rs")
text = path.read_text()
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
if needle not in text:
    raise SystemExit("queue_action authority insertion point not found")
path.write_text(text.replace(needle, replacement, 1))
