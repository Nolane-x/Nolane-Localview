use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use localview_live_bridge::ObserverEventKind;
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::{
    ControlState,
    source_map_runtime::{
        ProjectSourceMapError, ProjectSourceMapResponse, resolve_project_source_position,
    },
};

const MAX_RECENT_RUNTIME_EVENTS: usize = 2_048;
const MAX_RUNTIME_SOURCE_URL_BYTES: usize = 1_000;
const MAX_BROWSER_LINE: u32 = 1_000_000;
const MAX_BROWSER_COLUMN: u32 = 10_000_001;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeSourceRequest {
    event_seq: u64,
}

#[derive(Debug, Serialize)]
struct RuntimeSourceResponse {
    event_seq: u64,
    resolution: ProjectSourceMapResponse,
}

#[derive(Debug, Clone, Copy)]
enum RuntimeSourceError {
    Unauthorized,
    SessionNotFound,
    RuntimeEventNotFound,
    RuntimeEventKindUnsupported,
    RuntimeSourceMissing,
    RuntimeSourceUnsupported,
    RuntimeSourceAuthorityMismatch,
    RuntimePositionInvalid,
    Project(ProjectSourceMapError),
}

impl RuntimeSourceError {
    fn into_response(self) -> axum::response::Response {
        if let Self::Project(error) = self {
            return error.into_response();
        }

        let (status, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::SessionNotFound => (StatusCode::NOT_FOUND, "session_not_found"),
            Self::RuntimeEventNotFound => (StatusCode::NOT_FOUND, "runtime_event_not_found"),
            Self::RuntimeEventKindUnsupported => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "runtime_event_kind_unsupported",
            ),
            Self::RuntimeSourceMissing => {
                (StatusCode::UNPROCESSABLE_ENTITY, "runtime_source_missing")
            }
            Self::RuntimeSourceUnsupported => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "runtime_source_unsupported",
            ),
            Self::RuntimeSourceAuthorityMismatch => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "runtime_source_authority_mismatch",
            ),
            Self::RuntimePositionInvalid => {
                (StatusCode::UNPROCESSABLE_ENTITY, "runtime_position_invalid")
            }
            Self::Project(_) => unreachable!("project errors return above"),
        };

        (status, Json(serde_json::json!({ "error": code }))).into_response()
    }
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/runtime-source/resolve",
            post(resolve_runtime_source),
        )
        .with_state(state)
}

async fn resolve_runtime_source(
    State(state): State<ControlState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<SessionId>,
    Json(request): Json<RuntimeSourceRequest>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return RuntimeSourceError::Unauthorized.into_response();
    }

    match resolve_runtime_source_inner(&state, id, request.event_seq).await {
        Ok(response) => Json(response).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn resolve_runtime_source_inner(
    state: &ControlState,
    id: SessionId,
    event_seq: u64,
) -> Result<RuntimeSourceResponse, RuntimeSourceError> {
    let session = state
        .sessions
        .get(id)
        .await
        .ok_or(RuntimeSourceError::SessionNotFound)?;

    let event = state
        .live
        .recent(id, MAX_RECENT_RUNTIME_EVENTS)
        .await
        .into_iter()
        .find(|event| event.seq == event_seq)
        .ok_or(RuntimeSourceError::RuntimeEventNotFound)?;

    if event.kind != ObserverEventKind::RuntimeError {
        return Err(RuntimeSourceError::RuntimeEventKindUnsupported);
    }

    let source = event
        .payload
        .get("source")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= MAX_RUNTIME_SOURCE_URL_BYTES)
        .ok_or(RuntimeSourceError::RuntimeSourceMissing)?;

    let source_url =
        Url::parse(source).map_err(|_| RuntimeSourceError::RuntimeSourceUnsupported)?;
    if !matches!(source_url.scheme(), "http" | "https") {
        return Err(RuntimeSourceError::RuntimeSourceUnsupported);
    }
    if !source_url
        .scheme()
        .eq_ignore_ascii_case(session.endpoint.scheme.as_str())
    {
        return Err(RuntimeSourceError::RuntimeSourceAuthorityMismatch);
    }

    if !is_loopback_url(&source_url)
        || source_url.port_or_known_default() != Some(session.endpoint.port)
    {
        return Err(RuntimeSourceError::RuntimeSourceAuthorityMismatch);
    }

    let path = source_url.path();
    if path.contains('%') || path.starts_with("//") {
        return Err(RuntimeSourceError::RuntimeSourceUnsupported);
    }
    let generated_file = path
        .strip_prefix('/')
        .filter(|value| !value.is_empty())
        .ok_or(RuntimeSourceError::RuntimeSourceUnsupported)?;

    let browser_line = bounded_payload_u32(&event.payload, "line", 1, MAX_BROWSER_LINE)
        .ok_or(RuntimeSourceError::RuntimePositionInvalid)?;
    let browser_column = bounded_payload_u32(&event.payload, "column", 1, MAX_BROWSER_COLUMN)
        .ok_or(RuntimeSourceError::RuntimePositionInvalid)?;
    let generated_column = browser_column
        .checked_sub(1)
        .ok_or(RuntimeSourceError::RuntimePositionInvalid)?;

    let resolution = resolve_project_source_position(
        state,
        id,
        generated_file.to_owned(),
        browser_line,
        generated_column,
    )
    .await
    .map_err(RuntimeSourceError::Project)?;

    Ok(RuntimeSourceResponse {
        event_seq,
        resolution,
    })
}

fn bounded_payload_u32(payload: &serde_json::Value, key: &str, min: u32, max: u32) -> Option<u32> {
    let value = payload.get(key)?.as_u64()?;
    let value = u32::try_from(value).ok()?;
    (min..=max).contains(&value).then_some(value)
}

fn is_loopback_url(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_authority_is_conservative() {
        for value in [
            "http://localhost:5173/app.js",
            "http://127.0.0.1:5173/app.js",
            "http://127.42.0.9:5173/app.js",
            "http://[::1]:5173/app.js",
        ] {
            assert!(is_loopback_url(
                &Url::parse(value).expect("valid loopback URL")
            ));
        }

        for value in [
            "http://192.168.1.20:5173/app.js",
            "http://example.com:5173/app.js",
        ] {
            assert!(!is_loopback_url(
                &Url::parse(value).expect("valid non-loopback URL")
            ));
        }
    }

    #[test]
    fn payload_position_bounds_are_exact() {
        let payload = serde_json::json!({
            "line": 1,
            "column": 10_000_001
        });
        assert_eq!(bounded_payload_u32(&payload, "line", 1, 1_000_000), Some(1));
        assert_eq!(
            bounded_payload_u32(&payload, "column", 1, 10_000_001),
            Some(10_000_001)
        );
        assert_eq!(bounded_payload_u32(&payload, "missing", 1, 10), None);
    }
}
