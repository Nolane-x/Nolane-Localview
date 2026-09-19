use std::{
    net::IpAddr,
    time::Duration,
};

use axum::{
    extract::{OriginalUri, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_protocol::{Endpoint, Session, SessionId};
use localview_source_map::{ResolvedSourceLocation, SourceMap};
use reqwest::{header::HeaderValue, redirect::Policy, Client, Response};
use serde::Serialize;
use url::Url;

use crate::ControlState;

const RECENT_EVENT_LIMIT: usize = 2_048;
const MAX_RUNTIME_ERROR_CANDIDATES: usize = 8;
const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_MAP_BYTES: usize = 2 * 1024 * 1024;
const MAX_EXTERNAL_MAP_REFERENCE_BYTES: usize = 2_048;
const MAX_ANNOTATION_LINES: usize = 4;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1_500);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DiscoveryMethod {
    SourceMapHeader,
    XSourceMapHeader,
    ExternalAnnotation,
    InlineAnnotation,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UnresolvedReason {
    InvalidRuntimePosition,
    SourceOutsideSessionOrigin,
    SourceFetchFailed,
    SourceTooLarge,
    MapReferenceMissing,
    MapReferenceInvalid,
    MapOutsideSessionOrigin,
    MapFetchFailed,
    MapTooLarge,
    MapInvalid,
    PositionUnmapped,
}

#[derive(Debug, Serialize)]
struct RuntimeSourceResolutionResponse {
    records: Vec<RuntimeSourceResolutionRecord>,
}

#[derive(Debug, Serialize)]
struct RuntimeSourceResolutionRecord {
    observer_seq: u64,
    generated_source: Option<String>,
    generated_line: Option<u32>,
    generated_column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original: Option<ResolvedSourceResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_method: Option<DiscoveryMethod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved_reason: Option<UnresolvedReason>,
}

#[derive(Debug, Serialize)]
struct ResolvedSourceResponse {
    source: String,
    line: u32,
    column: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug)]
enum BoundedFetchError {
    Failed,
    TooLarge,
}

#[derive(Debug)]
enum MapReference {
    External {
        url: Url,
        method: DiscoveryMethod,
    },
    Inline {
        json: String,
        method: DiscoveryMethod,
    },
}

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/source/runtime-errors",
            get(session_runtime_source_resolution),
        )
        .with_state(state)
}

async fn session_runtime_source_resolution(
    State(state): State<ControlState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<SessionId>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return bounded_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    if uri.query().is_some() {
        return bounded_error(StatusCode::BAD_REQUEST, "query_not_allowed");
    }

    let Some(session) = state.sessions.get(id).await else {
        return bounded_error(StatusCode::NOT_FOUND, "session_not_found");
    };

    let client = match source_client() {
        Ok(client) => client,
        Err(()) => {
            return bounded_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "source_resolver_unavailable",
            )
        }
    };

    let events = state.live.recent(id, RECENT_EVENT_LIMIT).await;
    let candidates = events
        .iter()
        .rev()
        .filter(|event| event.kind == ObserverEventKind::RuntimeError)
        .filter(|event| event.payload.get("source").and_then(|value| value.as_str()).is_some())
        .take(MAX_RUNTIME_ERROR_CANDIDATES)
        .cloned()
        .collect::<Vec<_>>();

    let mut records = Vec::with_capacity(candidates.len());
    for event in candidates {
        records.push(resolve_runtime_error(&client, &session, &event).await);
    }

    Json(RuntimeSourceResolutionResponse { records }).into_response()
}

async fn resolve_runtime_error(
    client: &Client,
    session: &Session,
    event: &ObserverEvent,
) -> RuntimeSourceResolutionRecord {
    let raw_source = event
        .payload
        .get("source")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let retained_source = sanitize_retained_url(raw_source);
    let generated_line = event
        .payload
        .get("line")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0);
    let generated_column = event
        .payload
        .get("column")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .and_then(|value| value.checked_sub(1));

    let (generated_line, generated_column) = match (generated_line, generated_column) {
        (Some(line), Some(column)) => (line, column),
        _ => {
            return unresolved_record(
                event.seq,
                retained_source,
                generated_line,
                generated_column,
                UnresolvedReason::InvalidRuntimePosition,
            )
        }
    };

    let source_url = match validate_generated_source_url(&session.endpoint, raw_source) {
        Ok(url) => url,
        Err(reason) => {
            return unresolved_record(
                event.seq,
                retained_source,
                Some(generated_line),
                Some(generated_column),
                reason,
            )
        }
    };

    let map_reference = match discover_map_reference(client, &session.endpoint, &source_url).await {
        Ok(reference) => reference,
        Err(reason) => {
            return unresolved_record(
                event.seq,
                retained_source,
                Some(generated_line),
                Some(generated_column),
                reason,
            )
        }
    };

    let (map_json, method) = match map_reference {
        MapReference::Inline { json, method } => (json, method),
        MapReference::External { url, method } => {
            let bytes = match fetch_bounded_body(client, url, MAX_MAP_BYTES).await {
                Ok(bytes) => bytes,
                Err(BoundedFetchError::Failed) => {
                    return unresolved_record(
                        event.seq,
                        retained_source,
                        Some(generated_line),
                        Some(generated_column),
                        UnresolvedReason::MapFetchFailed,
                    )
                }
                Err(BoundedFetchError::TooLarge) => {
                    return unresolved_record(
                        event.seq,
                        retained_source,
                        Some(generated_line),
                        Some(generated_column),
                        UnresolvedReason::MapTooLarge,
                    )
                }
            };
            let Ok(json) = String::from_utf8(bytes) else {
                return unresolved_record(
                    event.seq,
                    retained_source,
                    Some(generated_line),
                    Some(generated_column),
                    UnresolvedReason::MapInvalid,
                );
            };
            (json, method)
        }
    };

    let map = match SourceMap::parse(&map_json) {
        Ok(map) => map,
        Err(_) => {
            return unresolved_record(
                event.seq,
                retained_source,
                Some(generated_line),
                Some(generated_column),
                UnresolvedReason::MapInvalid,
            )
        }
    };

    let Some(original) = map.resolve(generated_line, generated_column) else {
        return unresolved_record(
            event.seq,
            retained_source,
            Some(generated_line),
            Some(generated_column),
            UnresolvedReason::PositionUnmapped,
        );
    };

    resolved_record(
        event.seq,
        retained_source,
        generated_line,
        generated_column,
        method,
        original,
    )
}

async fn discover_map_reference(
    client: &Client,
    endpoint: &Endpoint,
    source_url: &Url,
) -> Result<MapReference, UnresolvedReason> {
    let mut response = send_exact_origin(client, endpoint, source_url.clone())
        .await
        .map_err(|_| UnresolvedReason::SourceFetchFailed)?;

    if let Some(length) = response.content_length() {
        if length > MAX_SOURCE_BYTES as u64 {
            return Err(UnresolvedReason::SourceTooLarge);
        }
    }

    if let Some((method, value)) = source_map_header(&response) {
        return parse_map_reference(endpoint, source_url, value, method);
    }

    let bytes = read_bounded_response(&mut response, MAX_SOURCE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedFetchError::Failed => UnresolvedReason::SourceFetchFailed,
            BoundedFetchError::TooLarge => UnresolvedReason::SourceTooLarge,
        })?;
    let source = String::from_utf8(bytes).map_err(|_| UnresolvedReason::SourceFetchFailed)?;
    let annotation =
        source_mapping_url_annotation(&source).ok_or(UnresolvedReason::MapReferenceMissing)?;

    let method = if annotation.starts_with("data:") {
        DiscoveryMethod::InlineAnnotation
    } else {
        DiscoveryMethod::ExternalAnnotation
    };
    parse_map_reference(endpoint, source_url, annotation, method)
}

fn source_map_header(response: &Response) -> Option<(DiscoveryMethod, &str)> {
    header_text(response.headers().get("sourcemap"))
        .map(|value| (DiscoveryMethod::SourceMapHeader, value))
        .or_else(|| {
            header_text(response.headers().get("x-sourcemap"))
                .map(|value| (DiscoveryMethod::XSourceMapHeader, value))
        })
}

fn header_text(value: Option<&HeaderValue>) -> Option<&str> {
    let value = value?.to_str().ok()?.trim();
    (!value.is_empty()).then_some(value)
}

fn source_mapping_url_annotation(source: &str) -> Option<&str> {
    source
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(MAX_ANNOTATION_LINES)
        .find_map(|line| {
            let line = line.trim();
            ["//# sourceMappingURL=", "//@ sourceMappingURL="]
                .into_iter()
                .find_map(|prefix| line.strip_prefix(prefix))
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
}

fn parse_map_reference(
    endpoint: &Endpoint,
    source_url: &Url,
    value: &str,
    method: DiscoveryMethod,
) -> Result<MapReference, UnresolvedReason> {
    if value.starts_with("data:") {
        return decode_inline_map(value, method);
    }
    if value.len() > MAX_EXTERNAL_MAP_REFERENCE_BYTES {
        return Err(UnresolvedReason::MapReferenceInvalid);
    }

    let mut url = source_url
        .join(value)
        .map_err(|_| UnresolvedReason::MapReferenceInvalid)?;
    url.set_fragment(None);
    validate_exact_origin_url(endpoint, &url)
        .map_err(|_| UnresolvedReason::MapOutsideSessionOrigin)?;

    Ok(MapReference::External { url, method })
}

fn decode_inline_map(
    value: &str,
    method: DiscoveryMethod,
) -> Result<MapReference, UnresolvedReason> {
    let (metadata, payload) = value
        .split_once(',')
        .ok_or(UnresolvedReason::MapReferenceInvalid)?;
    let metadata_lower = metadata.to_ascii_lowercase();
    if !metadata_lower.starts_with("data:application/json")
        || !metadata_lower.split(';').any(|part| part == "base64")
    {
        return Err(UnresolvedReason::MapReferenceInvalid);
    }

    let decoded = BASE64_STANDARD
        .decode(payload.as_bytes())
        .map_err(|_| UnresolvedReason::MapInvalid)?;
    if decoded.len() > MAX_MAP_BYTES {
        return Err(UnresolvedReason::MapTooLarge);
    }
    let json = String::from_utf8(decoded).map_err(|_| UnresolvedReason::MapInvalid)?;
    Ok(MapReference::Inline { json, method })
}

async fn fetch_bounded_body(
    client: &Client,
    url: Url,
    max_bytes: usize,
) -> Result<Vec<u8>, BoundedFetchError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| BoundedFetchError::Failed)?;

    if !response.status().is_success() {
        return Err(BoundedFetchError::Failed);
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(BoundedFetchError::TooLarge);
    }

    read_bounded_response(&mut response, max_bytes).await
}

async fn send_exact_origin(
    client: &Client,
    endpoint: &Endpoint,
    url: Url,
) -> Result<Response, ()> {
    validate_exact_origin_url(endpoint, &url).map_err(|_| ())?;
    let response = client.get(url).send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    Ok(response)
}

async fn read_bounded_response(
    response: &mut Response,
    max_bytes: usize,
) -> Result<Vec<u8>, BoundedFetchError> {
    let mut output = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(max_bytes),
    );

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| BoundedFetchError::Failed)?
    {
        let projected = output
            .len()
            .checked_add(chunk.len())
            .ok_or(BoundedFetchError::TooLarge)?;
        if projected > max_bytes {
            return Err(BoundedFetchError::TooLarge);
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

fn validate_generated_source_url(
    endpoint: &Endpoint,
    raw: &str,
) -> Result<Url, UnresolvedReason> {
    let url = Url::parse(raw).map_err(|_| UnresolvedReason::SourceOutsideSessionOrigin)?;
    validate_exact_origin_url(endpoint, &url)
        .map_err(|_| UnresolvedReason::SourceOutsideSessionOrigin)?;

    let path = url.path().to_ascii_lowercase();
    if ![".js", ".mjs", ".cjs", ".ts", ".tsx", ".jsx"]
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        return Err(UnresolvedReason::SourceOutsideSessionOrigin);
    }
    Ok(url)
}

fn validate_exact_origin_url(endpoint: &Endpoint, url: &Url) -> Result<(), ()> {
    if url.username() != "" || url.password().is_some() {
        return Err(());
    }
    if !matches!(url.scheme(), "http" | "https") || !url.scheme().eq_ignore_ascii_case(&endpoint.scheme)
    {
        return Err(());
    }
    let Some(host) = url.host_str() else {
        return Err(());
    };
    if !host.eq_ignore_ascii_case(&endpoint.host) || !is_loopback_host(host) {
        return Err(());
    }
    if url.port_or_known_default() != Some(endpoint.port) {
        return Err(());
    }
    if contains_redacted_marker(url.query()) {
        return Err(());
    }
    Ok(())
}

fn contains_redacted_marker(query: Option<&str>) -> bool {
    query.is_some_and(|query| {
        let lower = query.to_ascii_lowercase();
        lower.contains("[redacted]")
            || lower.contains("%5bredacted%5d")
            || lower.contains("%5b%52%45%44%41%43%54%45%44%5d")
    })
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

fn sanitize_retained_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw).ok()?;
    if url.username() != "" || url.password().is_some() {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

fn source_client() -> Result<Client, ()> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| ())
}

fn resolved_record(
    observer_seq: u64,
    generated_source: Option<String>,
    generated_line: u32,
    generated_column: u32,
    discovery_method: DiscoveryMethod,
    original: ResolvedSourceLocation,
) -> RuntimeSourceResolutionRecord {
    RuntimeSourceResolutionRecord {
        observer_seq,
        generated_source,
        generated_line: Some(generated_line),
        generated_column: Some(generated_column),
        original: Some(ResolvedSourceResponse {
            source: original.source,
            line: original.line,
            column: original.column,
            name: original.name,
        }),
        discovery_method: Some(discovery_method),
        unresolved_reason: None,
    }
}

fn unresolved_record(
    observer_seq: u64,
    generated_source: Option<String>,
    generated_line: Option<u32>,
    generated_column: Option<u32>,
    unresolved_reason: UnresolvedReason,
) -> RuntimeSourceResolutionRecord {
    RuntimeSourceResolutionRecord {
        observer_seq,
        generated_source,
        generated_line,
        generated_column,
        original: None,
        discovery_method: None,
        unresolved_reason: Some(unresolved_reason),
    }
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    let expected = format!("Bearer {}", state.token);
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn bounded_error(status: StatusCode, code: &'static str) -> axum::response::Response {
    (status, Json(serde_json::json!({ "error": code }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> Endpoint {
        Endpoint {
            host: "127.0.0.1".into(),
            port: 5173,
            scheme: "http".into(),
        }
    }

    #[test]
    fn exact_origin_rejects_alias_port_scheme_credentials_and_redacted_query() {
        let endpoint = endpoint();
        assert!(validate_exact_origin_url(
            &endpoint,
            &Url::parse("http://127.0.0.1:5173/app.js?t=1").unwrap()
        )
        .is_ok());

        for url in [
            "http://localhost:5173/app.js",
            "http://127.0.0.2:5173/app.js",
            "http://127.0.0.1:5174/app.js",
            "https://127.0.0.1:5173/app.js",
            "http://user@127.0.0.1:5173/app.js",
            "http://127.0.0.1:5173/app.js?token=%5BREDACTED%5D",
        ] {
            assert!(
                validate_exact_origin_url(&endpoint, &Url::parse(url).unwrap()).is_err(),
                "{url} must not inherit session source authority"
            );
        }
    }

    #[test]
    fn generated_source_requires_script_like_path() {
        let endpoint = endpoint();
        assert!(validate_generated_source_url(
            &endpoint,
            "http://127.0.0.1:5173/src/App.tsx?t=123"
        )
        .is_ok());
        assert!(validate_generated_source_url(
            &endpoint,
            "http://127.0.0.1:5173/api/source"
        )
        .is_err());
    }

    #[test]
    fn annotation_is_standalone_and_limited_to_final_nonempty_lines() {
        let source = [
            "const decoy = '//# sourceMappingURL=decoy.map';",
            "",
            "console.log('x');",
            "",
            "//# sourceMappingURL=real.map",
        ]
        .join("\n");
        assert_eq!(source_mapping_url_annotation(&source), Some("real.map"));

        let too_early = [
            "//# sourceMappingURL=early.map",
            "one",
            "two",
            "three",
            "four",
            "five",
        ]
        .join("\n");
        assert_eq!(source_mapping_url_annotation(&too_early), None);
    }

    #[test]
    fn external_map_reference_cannot_escape_session_origin() {
        let endpoint = endpoint();
        let source = Url::parse("http://127.0.0.1:5173/assets/app.js").unwrap();

        let valid = parse_map_reference(
            &endpoint,
            &source,
            "./app.js.map",
            DiscoveryMethod::ExternalAnnotation,
        )
        .unwrap();
        match valid {
            MapReference::External { url, .. } => {
                assert_eq!(url.as_str(), "http://127.0.0.1:5173/assets/app.js.map");
            }
            MapReference::Inline { .. } => panic!("expected external map"),
        }

        assert!(matches!(
            parse_map_reference(
                &endpoint,
                &source,
                "http://127.0.0.1:9999/app.js.map",
                DiscoveryMethod::ExternalAnnotation,
            ),
            Err(UnresolvedReason::MapOutsideSessionOrigin)
        ));
    }

    #[test]
    fn inline_base64_map_is_bounded_and_decoded_without_retaining_data_url() {
        let map_json = r#"{"version":3,"sources":["src/App.tsx"],"names":[],"mappings":"AAAA"}"#;
        let encoded = BASE64_STANDARD.encode(map_json);
        let reference = format!("data:application/json;charset=utf-8;base64,{encoded}");

        let decoded =
            decode_inline_map(&reference, DiscoveryMethod::InlineAnnotation).unwrap();
        match decoded {
            MapReference::Inline { json, method } => {
                assert_eq!(json, map_json);
                assert_eq!(method, DiscoveryMethod::InlineAnnotation);
            }
            MapReference::External { .. } => panic!("expected inline map"),
        }
    }
}
