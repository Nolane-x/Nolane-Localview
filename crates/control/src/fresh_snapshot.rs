use std::{collections::BTreeMap, time::Duration};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use localview_live_bridge::{BridgeActionKind, BridgeActionResult};
use localview_protocol::{PageSnapshot, Rect, SemanticNode, SessionId, SourceLocation};
use serde_json::Value;
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::ControlState;

const FRESH_SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(650);
const ACTION_RESULT_POLL: Duration = Duration::from_millis(20);
const MAX_SEMANTIC_NODES: usize = 600;
const MAX_TREE_DEPTH: usize = 12;
const MAX_VIEWPORT_DIMENSION: u64 = 100_000;
const MAX_ROUTE_BYTES: usize = 1_000;
const MAX_REFERENCE_BYTES: usize = 256;
const MAX_TAG_BYTES: usize = 64;
const MAX_ROLE_BYTES: usize = 96;
const MAX_NAME_BYTES: usize = 256;
const MAX_ATTRIBUTE_ENTRIES: usize = 64;
const MAX_ATTRIBUTE_KEY_BYTES: usize = 128;
const MAX_ATTRIBUTE_VALUE_BYTES: usize = 256;
const MAX_SOURCE_FILE_BYTES: usize = 260;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/semantic-snapshot/fresh",
            get(session_fresh_semantic_snapshot),
        )
        .with_state(state)
}

async fn session_fresh_semantic_snapshot(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if state.sessions.get(id).await.is_none() {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    }

    let action = state
        .live
        .enqueue_action(id, None, BridgeActionKind::Snapshot)
        .await;
    let Some(result) = wait_for_matching_result(&state, id, action.id).await else {
        return bounded_error(
            StatusCode::GATEWAY_TIMEOUT,
            "fresh_semantic_snapshot_timeout",
        );
    };
    if !result.ok {
        return bounded_error(StatusCode::BAD_GATEWAY, "fresh_semantic_snapshot_failed");
    }

    let Some(snapshot) = project_snapshot(&result.payload, result.completed_at) else {
        return bounded_error(
            StatusCode::BAD_GATEWAY,
            "invalid_fresh_semantic_snapshot",
        );
    };

    Json(snapshot).into_response()
}

async fn wait_for_matching_result(
    state: &ControlState,
    session_id: SessionId,
    action_id: Uuid,
) -> Option<BridgeActionResult> {
    let deadline = Instant::now() + FRESH_SNAPSHOT_TIMEOUT;
    loop {
        if let Some(result) = state
            .live
            .recent_results(session_id, 64)
            .await
            .into_iter()
            .rev()
            .find(|result| result.action_id == action_id)
        {
            return Some(result);
        }

        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        sleep(ACTION_RESULT_POLL.min(deadline.saturating_duration_since(now))).await;
    }
}

fn project_snapshot(payload: &Value, captured_at: DateTime<Utc>) -> Option<PageSnapshot> {
    let version = payload.get("version")?.as_u64()?;
    let route = bounded_required_string(payload.get("route")?, MAX_ROUTE_BYTES)?;
    let viewport = project_viewport(payload.get("viewport")?)?;
    let raw_root = payload.get("semantic_tree")?;
    if raw_root.is_null() {
        return None;
    }

    let mut remaining_nodes = MAX_SEMANTIC_NODES;
    let root = project_node(raw_root, 0, &mut remaining_nodes)?;

    Some(PageSnapshot {
        version,
        route,
        viewport,
        root,
        console_errors: Vec::new(),
        failed_requests: Vec::new(),
        captured_at,
    })
}

fn project_viewport(value: &Value) -> Option<(u32, u32)> {
    let width = value.get("width")?.as_u64()?;
    let height = value.get("height")?.as_u64()?;
    if width == 0
        || height == 0
        || width > MAX_VIEWPORT_DIMENSION
        || height > MAX_VIEWPORT_DIMENSION
    {
        return None;
    }
    Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?))
}

fn project_node(value: &Value, depth: usize, remaining: &mut usize) -> Option<SemanticNode> {
    if depth > MAX_TREE_DEPTH || *remaining == 0 {
        return None;
    }
    *remaining -= 1;

    let reference = bounded_required_string(value.get("ref")?, MAX_REFERENCE_BYTES)?;
    let tag = bounded_required_string(value.get("tag")?, MAX_TAG_BYTES)?;
    let role = bounded_optional_string(value.get("role"), MAX_ROLE_BYTES)?;
    let name = bounded_optional_string(value.get("name"), MAX_NAME_BYTES)?;
    let rect = match value.get("rect") {
        None | Some(Value::Null) => None,
        Some(raw) => Some(project_rect(raw)?),
    };
    let interactive = value.get("interactive")?.as_bool()?;
    let attributes = project_attributes(value.get("attributes"))?;
    let source = project_source(value.get("sourceHint"))?;

    let raw_children = value.get("children")?.as_array()?;
    let mut children = Vec::with_capacity(raw_children.len().min(*remaining));
    for child in raw_children {
        children.push(project_node(child, depth + 1, remaining)?);
    }

    Some(SemanticNode {
        reference,
        role,
        name,
        tag,
        rect,
        interactive,
        attributes,
        source,
        children,
    })
}

fn project_rect(value: &Value) -> Option<Rect> {
    let x = value.get("x")?.as_f64()?;
    let y = value.get("y")?.as_f64()?;
    let width = value.get("width")?.as_f64()?;
    let height = value.get("height")?.as_f64()?;
    if !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || !(x + width).is_finite()
        || !(y + height).is_finite()
        || width < 0.0
        || height < 0.0
    {
        return None;
    }
    Some(Rect {
        x,
        y,
        width,
        height,
    })
}

fn project_attributes(value: Option<&Value>) -> Option<BTreeMap<String, String>> {
    let Some(value) = value else {
        return Some(BTreeMap::new());
    };
    let map = value.as_object()?;
    if map.len() > MAX_ATTRIBUTE_ENTRIES {
        return None;
    }

    let mut output = BTreeMap::new();
    for (key, value) in map {
        if key.is_empty() || key.len() > MAX_ATTRIBUTE_KEY_BYTES {
            return None;
        }
        let value = value.as_str()?;
        if value.len() > MAX_ATTRIBUTE_VALUE_BYTES {
            return None;
        }
        output.insert(key.clone(), value.to_owned());
    }
    Some(output)
}

fn project_source(value: Option<&Value>) -> Option<Option<SourceLocation>> {
    let Some(value) = value else {
        return Some(None);
    };
    if value.is_null() {
        return Some(None);
    }

    let origin = bounded_required_string(value.get("origin")?, 64)?;
    if !matches!(origin.as_str(), "data-component-source" | "data-source") {
        return None;
    }
    let file = bounded_required_string(value.get("file")?, MAX_SOURCE_FILE_BYTES)?;
    let line = u32::try_from(value.get("line")?.as_u64()?).ok()?;
    let column = match value.get("column") {
        None | Some(Value::Null) => None,
        Some(raw) => Some(u32::try_from(raw.as_u64()?).ok()?),
    };
    // `data-component-source` is an explicit ownership hint, but a file alone is not a
    // component identity: one file may contain many components. Bind the identity to the
    // declared source line as well so only ancestors carrying the same explicit component
    // source location can corroborate ownership. Column stays diagnostic, not identity.
    let component =
        (origin == "data-component-source").then(|| format!("{file}:{line}"));

    Some(Some(SourceLocation {
        file,
        line,
        column,
        component,
    }))
}

fn bounded_required_string(value: &Value, max_bytes: usize) -> Option<String> {
    let value = value.as_str()?;
    if value.is_empty() || value.len() > max_bytes {
        return None;
    }
    Some(value.to_owned())
}

fn bounded_optional_string(value: Option<&Value>, max_bytes: usize) -> Option<Option<String>> {
    let Some(value) = value else {
        return Some(None);
    };
    if value.is_null() {
        return Some(None);
    }
    let value = value.as_str()?;
    if value.len() > max_bytes {
        return None;
    }
    Some(Some(value.to_owned()))
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn bounded_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(serde_json::json!({"error": code}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_source_hint_never_becomes_component_ownership() {
        let source = serde_json::json!({
            "origin": "data-source",
            "file": "button.tsx",
            "line": 7,
            "column": 2
        });
        let projected = project_source(Some(&source))
            .expect("valid source hint")
            .expect("source location");
        assert_eq!(projected.file, "button.tsx");
        assert_eq!(projected.component, None);
    }

    #[test]
    fn component_source_hint_preserves_explicit_component_evidence() {
        let source = serde_json::json!({
            "origin": "data-component-source",
            "file": "SettingsCard.tsx",
            "line": 10,
            "column": null
        });
        let projected = project_source(Some(&source))
            .expect("valid component source hint")
            .expect("source location");
        assert_eq!(projected.component.as_deref(), Some("SettingsCard.tsx:10"));
    }
}
