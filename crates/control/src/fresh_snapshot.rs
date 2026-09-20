use std::{collections::BTreeMap, time::Duration};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use chrono::{DateTime, Utc};
use localview_live_bridge::{BridgeActionKind, BridgeActionResult};
use localview_protocol::{
    ComponentOwnership, PageSnapshot, Rect, SemanticNode, SessionId, SourceLocation,
};
use serde_json::Value;
use tokio::time::{Instant, sleep};
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
const MAX_REACT_COMPONENT_BYTES: usize = 96;
const MAX_REACT_COMPONENT_ID_BYTES: usize = 384;
const MAX_REACT_SOURCE_LINE: u32 = 1_000_000;
const MAX_REACT_SOURCE_COLUMN: u32 = 10_000_001;
const MAX_SVELTE_COMPONENT_BYTES: usize = 96;
const MAX_SVELTE_COMPONENT_ID_BYTES: usize = 384;
const MAX_SVELTE_SOURCE_LINE: u32 = 1_000_000;
const MAX_SVELTE_SOURCE_COLUMN: u32 = 10_000_000;
const MAX_VUE_COMPONENT_BYTES: usize = 96;
const MAX_COMPONENT_SIGNAL_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreshSnapshotError {
    SessionNotFound,
    Timeout,
    Failed,
    Invalid,
}

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

    match acquire_fresh_semantic_snapshot(&state, id).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(FreshSnapshotError::SessionNotFound) => {
            bounded_error(StatusCode::NOT_FOUND, "session_not_found")
        }
        Err(FreshSnapshotError::Timeout) => bounded_error(
            StatusCode::GATEWAY_TIMEOUT,
            "fresh_semantic_snapshot_timeout",
        ),
        Err(FreshSnapshotError::Failed) => {
            bounded_error(StatusCode::BAD_GATEWAY, "fresh_semantic_snapshot_failed")
        }
        Err(FreshSnapshotError::Invalid) => {
            bounded_error(StatusCode::BAD_GATEWAY, "invalid_fresh_semantic_snapshot")
        }
    }
}

pub(crate) async fn acquire_fresh_snapshot_result(
    state: &ControlState,
    id: SessionId,
) -> Result<BridgeActionResult, FreshSnapshotError> {
    if state.sessions.get(id).await.is_none() {
        return Err(FreshSnapshotError::SessionNotFound);
    }

    let action = state
        .live
        .enqueue_action(id, None, BridgeActionKind::Snapshot)
        .await;
    let result = wait_for_matching_result(state, id, action.id)
        .await
        .ok_or(FreshSnapshotError::Timeout)?;
    if !result.ok {
        return Err(FreshSnapshotError::Failed);
    }
    Ok(result)
}

pub(crate) async fn acquire_fresh_semantic_snapshot(
    state: &ControlState,
    id: SessionId,
) -> Result<PageSnapshot, FreshSnapshotError> {
    let result = acquire_fresh_snapshot_result(state, id).await?;
    project_snapshot(&result.payload, result.completed_at).ok_or(FreshSnapshotError::Invalid)
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
    let ownership = project_ownership(value.get("sourceHint"))?;

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
        ownership,
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
    if !matches!(
        origin.as_str(),
        "data-component-source"
            | "data-source"
            | "react-dev-fiber"
            | "svelte-dev-meta"
            | "vue-dev-instance"
    ) {
        return None;
    }
    if origin == "vue-dev-instance" {
        return Some(None);
    }
    let file = bounded_required_string(value.get("file")?, MAX_SOURCE_FILE_BYTES)?;
    let line = u32::try_from(value.get("line")?.as_u64()?).ok()?;
    let column = match value.get("column") {
        None | Some(Value::Null) => None,
        Some(raw) => Some(u32::try_from(raw.as_u64()?).ok()?),
    };

    let component = match origin.as_str() {
        "data-component-source" => {
            // Explicit ownership is bound to source location because one file may contain
            // multiple components. Column remains diagnostic rather than identity.
            Some(format!("{file}:{line}"))
        }
        "react-dev-fiber" => {
            if line == 0 || line > MAX_REACT_SOURCE_LINE {
                return None;
            }
            if column.is_some_and(|value| value == 0 || value > MAX_REACT_SOURCE_COLUMN) {
                return None;
            }
            let component_name =
                bounded_required_string(value.get("component")?, MAX_REACT_COMPONENT_BYTES)?;
            let identity = format!("react:{file}:{component_name}");
            if identity.len() > MAX_REACT_COMPONENT_ID_BYTES {
                return None;
            }
            Some(identity)
        }
        "svelte-dev-meta" => {
            if !valid_svelte_relative_file(&file)
                || line == 0
                || line > MAX_SVELTE_SOURCE_LINE
                || column.is_none_or(|value| value > MAX_SVELTE_SOURCE_COLUMN)
            {
                return None;
            }
            let component_name =
                bounded_required_string(value.get("component")?, MAX_SVELTE_COMPONENT_BYTES)?;
            let identity = format!("svelte:{file}:{component_name}");
            if identity.len() > MAX_SVELTE_COMPONENT_ID_BYTES {
                return None;
            }
            Some(identity)
        }
        "data-source" => None,
        _ => unreachable!("source origin was validated above"),
    };

    Some(Some(SourceLocation {
        file,
        line,
        column,
        component,
    }))
}

fn project_ownership(value: Option<&Value>) -> Option<Option<ComponentOwnership>> {
    let Some(value) = value else {
        return Some(None);
    };
    if value.is_null() {
        return Some(None);
    }

    let origin = bounded_required_string(value.get("origin")?, 64)?;
    match origin.as_str() {
        "data-source" => Some(None),
        "data-component-source" => {
            let source = project_source(Some(value))??;
            let component = source.component.clone()?;
            Some(Some(ComponentOwnership {
                framework: None,
                file: source.file,
                component,
                signal: "data_component_source".into(),
            }))
        }
        "react-dev-fiber" => {
            let source = project_source(Some(value))??;
            let component =
                bounded_required_string(value.get("component")?, MAX_REACT_COMPONENT_BYTES)?;
            let Some(signal_value) = value.get("signal") else {
                return Some(None);
            };
            let Some(signal) = bounded_required_string(signal_value, MAX_COMPONENT_SIGNAL_BYTES)
            else {
                return Some(None);
            };
            if !matches!(signal.as_str(), "debug_source" | "debug_stack") {
                return Some(None);
            }
            Some(Some(ComponentOwnership {
                framework: Some("react".into()),
                file: source.file,
                component,
                signal,
            }))
        }
        "svelte-dev-meta" => {
            let source = project_source(Some(value))??;
            let component =
                bounded_required_string(value.get("component")?, MAX_SVELTE_COMPONENT_BYTES)?;
            let Some(signal_value) = value.get("signal") else {
                return Some(None);
            };
            let Some(signal) = bounded_required_string(signal_value, MAX_COMPONENT_SIGNAL_BYTES)
            else {
                return Some(None);
            };
            if signal != "element_meta" {
                return Some(None);
            }
            Some(Some(ComponentOwnership {
                framework: Some("svelte".into()),
                file: source.file,
                component,
                signal,
            }))
        }
        "vue-dev-instance" => {
            let Some(file) = value
                .get("file")
                .and_then(|value| bounded_required_string(value, MAX_SOURCE_FILE_BYTES))
            else {
                return Some(None);
            };
            if !valid_vue_relative_file(&file) {
                return Some(None);
            }
            let Some(component) = value
                .get("component")
                .and_then(|value| bounded_required_string(value, MAX_VUE_COMPONENT_BYTES))
            else {
                return Some(None);
            };
            let Some(signal) = value
                .get("signal")
                .and_then(|value| bounded_required_string(value, MAX_COMPONENT_SIGNAL_BYTES))
            else {
                return Some(None);
            };
            if signal != "element_parent_component" {
                return Some(None);
            }
            Some(Some(ComponentOwnership {
                framework: Some("vue".into()),
                file,
                component,
                signal,
            }))
        }
        _ => None,
    }
}

fn valid_svelte_relative_file(file: &str) -> bool {
    valid_framework_relative_file(file, ".svelte")
}

fn valid_vue_relative_file(file: &str) -> bool {
    valid_framework_relative_file(file, ".vue")
}

fn valid_framework_relative_file(file: &str, extension: &str) -> bool {
    if !file.ends_with(extension)
        || file.starts_with('/')
        || file.contains('\\')
        || file
            .chars()
            .any(|character| matches!(character, '%' | '?' | '#' | ':'))
        || file.chars().any(char::is_control)
    {
        return false;
    }

    let mut saw_segment = false;
    for segment in file.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return false;
        }
        saw_segment = true;
    }
    saw_segment
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

    #[test]
    fn react_dev_fiber_hint_preserves_bounded_component_evidence() {
        let source = serde_json::json!({
            "origin": "react-dev-fiber",
            "file": "src/SettingsCard.tsx",
            "line": 17,
            "column": 5,
            "component": "SettingsCard"
        });
        let projected = project_source(Some(&source))
            .expect("valid React source hint")
            .expect("source location");
        assert_eq!(projected.file, "src/SettingsCard.tsx");
        assert_eq!(projected.line, 17);
        assert_eq!(projected.column, Some(5));
        assert_eq!(
            projected.component.as_deref(),
            Some("react:src/SettingsCard.tsx:SettingsCard")
        );
    }

    #[test]
    fn react_component_identity_is_stable_across_host_jsx_lines() {
        let first = serde_json::json!({
            "origin": "react-dev-fiber",
            "file": "src/SettingsCard.tsx",
            "line": 17,
            "column": 5,
            "component": "SettingsCard"
        });
        let second = serde_json::json!({
            "origin": "react-dev-fiber",
            "file": "src/SettingsCard.tsx",
            "line": 29,
            "column": 9,
            "component": "SettingsCard"
        });

        let first = project_source(Some(&first))
            .expect("valid first React source")
            .expect("first source location");
        let second = project_source(Some(&second))
            .expect("valid second React source")
            .expect("second source location");

        assert_eq!(first.component, second.component);
        assert_ne!(first.line, second.line);
        assert_eq!(
            first.component.as_deref(),
            Some("react:src/SettingsCard.tsx:SettingsCard")
        );
    }

    #[test]
    fn svelte_dev_meta_hint_preserves_real_zero_based_source_coordinates() {
        let source = serde_json::json!({
            "origin": "svelte-dev-meta",
            "file": "src/SvelteCard.svelte",
            "line": 17,
            "column": 0,
            "component": "SvelteCard"
        });
        let projected = project_source(Some(&source))
            .expect("valid Svelte source hint")
            .expect("source location");
        assert_eq!(projected.file, "src/SvelteCard.svelte");
        assert_eq!(projected.line, 17);
        assert_eq!(projected.column, Some(0));
        assert_eq!(
            projected.component.as_deref(),
            Some("svelte:src/SvelteCard.svelte:SvelteCard")
        );
    }

    #[test]
    fn svelte_component_identity_is_stable_across_element_locations() {
        let first = serde_json::json!({
            "origin": "svelte-dev-meta",
            "file": "src/SvelteCard.svelte",
            "line": 7,
            "column": 0,
            "component": "SvelteCard"
        });
        let second = serde_json::json!({
            "origin": "svelte-dev-meta",
            "file": "src/SvelteCard.svelte",
            "line": 23,
            "column": 4,
            "component": "SvelteCard"
        });

        let first = project_source(Some(&first))
            .expect("valid first Svelte source")
            .expect("first source location");
        let second = project_source(Some(&second))
            .expect("valid second Svelte source")
            .expect("second source location");

        assert_eq!(first.component, second.component);
        assert_ne!(first.line, second.line);
        assert_eq!(
            first.component.as_deref(),
            Some("svelte:src/SvelteCard.svelte:SvelteCard")
        );
    }

    #[test]
    fn svelte_dev_meta_hint_fails_closed_for_unsafe_or_incomplete_identity() {
        for source in [
            serde_json::json!({
                "origin": "svelte-dev-meta",
                "file": "/private/SvelteCard.svelte",
                "line": 1,
                "column": 0,
                "component": "SvelteCard"
            }),
            serde_json::json!({
                "origin": "svelte-dev-meta",
                "file": "src/../SvelteCard.svelte",
                "line": 1,
                "column": 0,
                "component": "SvelteCard"
            }),
            serde_json::json!({
                "origin": "svelte-dev-meta",
                "file": "src/SvelteCard.svelte",
                "line": 0,
                "column": 0,
                "component": "SvelteCard"
            }),
            serde_json::json!({
                "origin": "svelte-dev-meta",
                "file": "src/SvelteCard.svelte",
                "line": 1,
                "component": "SvelteCard"
            }),
            serde_json::json!({
                "origin": "svelte-dev-meta",
                "file": "src/SvelteCard.svelte",
                "line": 1,
                "column": 10_000_001,
                "component": "SvelteCard"
            }),
            serde_json::json!({
                "origin": "svelte-dev-meta",
                "file": "src/SvelteCard.svelte",
                "line": 1,
                "column": 0
            }),
        ] {
            assert!(
                project_source(Some(&source)).is_none(),
                "invalid Svelte ownership must fail closed"
            );
        }
    }

    #[test]
    fn react_dev_fiber_hint_fails_closed_without_valid_identity() {
        for source in [
            serde_json::json!({
                "origin": "react-dev-fiber",
                "file": "src/App.tsx",
                "line": 0,
                "column": 1,
                "component": "App"
            }),
            serde_json::json!({
                "origin": "react-dev-fiber",
                "file": "src/App.tsx",
                "line": 1,
                "column": 0,
                "component": "App"
            }),
            serde_json::json!({
                "origin": "react-dev-fiber",
                "file": "src/App.tsx",
                "line": 1,
                "column": 1
            }),
        ] {
            assert!(
                project_source(Some(&source)).is_none(),
                "invalid React ownership must fail closed"
            );
        }
    }
    #[test]
    fn vue_file_only_ownership_never_fabricates_source_location() {
        let hint = serde_json::json!({
            "origin": "vue-dev-instance",
            "file": "src/VueCard.vue",
            "component": "VueCard",
            "signal": "element_parent_component"
        });

        assert_eq!(project_source(Some(&hint)), Some(None));
        let ownership = project_ownership(Some(&hint))
            .expect("valid Vue ownership")
            .expect("Vue component ownership");
        assert_eq!(ownership.framework.as_deref(), Some("vue"));
        assert_eq!(ownership.file, "src/VueCard.vue");
        assert_eq!(ownership.component, "VueCard");
        assert_eq!(ownership.signal, "element_parent_component");
    }

    #[test]
    fn vue_file_only_ownership_fails_closed_for_unsafe_identity() {
        for file in [
            "/private/VueCard.vue",
            "src/../VueCard.vue",
            "src/%2e%2e/VueCard.vue",
            "https://example.test/VueCard.vue",
            "src/VueCard.ts",
        ] {
            let hint = serde_json::json!({
                "origin": "vue-dev-instance",
                "file": file,
                "component": "VueCard",
                "signal": "element_parent_component"
            });
            assert_eq!(
                project_ownership(Some(&hint)),
                Some(None),
                "unsafe Vue ownership must be dropped without invalidating the snapshot: {file}"
            );
        }
    }
}
