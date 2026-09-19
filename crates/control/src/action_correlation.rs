#![forbid(unsafe_code)]

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use localview_causal::{
    correlate_action_request_ui, ActionCorrelationWindow, ActionRequestUiPolicy, RuntimeSignal,
    RuntimeSignalKind,
};
use localview_evidence::{
    EvidenceDraft, EvidenceKind, EvidenceObject, Provenance, UncertaintyClass,
};
use localview_protocol::SessionId;
use uuid::Uuid;

use crate::ControlState;

const MAX_CORRELATION_EVIDENCE_SCAN: usize = 512;

pub(crate) fn router(state: ControlState) -> Router {
    Router::new()
        .route(
            "/v1/sessions/{id}/actions/{action_id}/correlation",
            get(action_correlation),
        )
        .with_state(state)
}

fn authorized(headers: &HeaderMap, state: &ControlState) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == state.token.as_ref())
}

fn denied() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": "unauthorized"})),
    )
        .into_response()
}

fn session_not_found() -> axum::response::Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "session_not_found"})),
    )
        .into_response()
}

fn timestamp_millis(value: DateTime<Utc>) -> u64 {
    u64::try_from(value.timestamp_millis()).unwrap_or(0)
}

fn signal_kind(evidence: &EvidenceObject) -> Option<RuntimeSignalKind> {
    match evidence.payload.get("kind").and_then(|value| value.as_str()) {
        Some("network") if evidence.kind == EvidenceKind::Network => Some(RuntimeSignalKind::Network),
        Some("dom_mutation") if evidence.kind == EvidenceKind::Semantic => {
            Some(RuntimeSignalKind::DomMutation)
        }
        Some("layout") if evidence.kind == EvidenceKind::Layout => Some(RuntimeSignalKind::Layout),
        Some("route") if evidence.kind == EvidenceKind::Interaction => Some(RuntimeSignalKind::Route),
        _ => None,
    }
}

fn runtime_signal(evidence: &EvidenceObject) -> Option<RuntimeSignal> {
    Some(RuntimeSignal {
        id: evidence.id.clone(),
        kind: signal_kind(evidence)?,
        observed_ms: timestamp_millis(evidence.provenance.captured_at),
        route: evidence
            .payload
            .get("route")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        reference: evidence.region.clone(),
    })
}

fn action_parent(
    evidence: &[EvidenceObject],
    action_id: Uuid,
) -> Option<&EvidenceObject> {
    let expected = action_id.to_string();
    evidence.iter().rev().find(|item| {
        item.kind == EvidenceKind::Interaction
            && item
                .payload
                .get("action_id")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == expected)
    })
}

async fn action_correlation(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path((id, action_id)): Path<(SessionId, Uuid)>,
) -> axum::response::Response {
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(id).await.is_none() {
        return session_not_found();
    }

    let Some(boundary) = state.live.action_execution_boundary(id, action_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "completed_action_boundary_not_found"})),
        )
            .into_response();
    };

    let policy = ActionRequestUiPolicy::default();
    let started_ms = timestamp_millis(boundary.started_at);
    let completed_ms = timestamp_millis(boundary.completed_at);
    let closes_ms = completed_ms.saturating_add(policy.tail_ms);
    if timestamp_millis(Utc::now()) < closes_ms {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "correlation_window_open",
                "action_id": action_id,
            })),
        )
            .into_response();
    }

    let evidence = state
        .evidence
        .recent_for_session(id, MAX_CORRELATION_EVIDENCE_SCAN)
        .await;
    let Some(action_evidence) = action_parent(&evidence, action_id) else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "action_evidence_missing"})),
        )
            .into_response();
    };

    let signals = evidence.iter().filter_map(runtime_signal).collect::<Vec<_>>();
    let window = ActionCorrelationWindow {
        action_id: action_id.to_string(),
        started_ms,
        completed_ms,
        route: None,
    };
    let trace = match correlate_action_request_ui(&window, &signals, &policy) {
        Ok(trace) => trace,
        Err(_) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": "correlation_policy_rejected"})),
            )
                .into_response();
        }
    };

    let mut parent_ids = vec![action_evidence.id.clone()];
    for link in &trace.links {
        parent_ids.push(link.request_id.clone());
        parent_ids.extend(link.response_ids.iter().cloned());
    }
    parent_ids.sort();
    parent_ids.dedup();

    let confidence = trace
        .links
        .iter()
        .map(|link| link.confidence)
        .fold(0.0_f32, f32::max);
    let captured_at = boundary.completed_at
        + chrono::Duration::milliseconds(i64::try_from(policy.tail_ms).unwrap_or(0));
    let stored = state
        .evidence
        .insert(EvidenceDraft {
            kind: EvidenceKind::Causal,
            session_id: id,
            region: action_evidence.region.clone(),
            payload: serde_json::to_value(&trace).unwrap_or(serde_json::Value::Null),
            provenance: Provenance {
                source: "wave3-action-request-ui-correlation".into(),
                engine: Some("deterministic-temporal-window".into()),
                revision: action_evidence.provenance.revision.clone(),
                parent_ids,
                captured_at,
            },
            confidence,
            uncertainty: UncertaintyClass::Derived,
            secret_taint: false,
        })
        .await;

    Json(serde_json::json!({
        "trace": trace,
        "evidence_id": stored.id,
        "deduplicated": stored.deduplicated,
        "window": {
            "started_at": boundary.started_at,
            "completed_at": boundary.completed_at,
            "basis": "daemon_execution_boundary"
        }
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(
        kind: EvidenceKind,
        event_kind: &str,
        id: &str,
        captured_at: DateTime<Utc>,
    ) -> EvidenceObject {
        EvidenceObject {
            id: id.into(),
            kind,
            session_id: Uuid::nil(),
            region: Some("@e1".into()),
            payload: serde_json::json!({
                "kind": event_kind,
                "route": "http://127.0.0.1:5173/"
            }),
            provenance: Provenance {
                source: "test".into(),
                engine: None,
                revision: None,
                parent_ids: Vec::new(),
                captured_at,
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        }
    }

    #[test]
    fn projects_only_allowed_runtime_signal_classes() {
        let now = Utc::now();
        assert_eq!(
            runtime_signal(&evidence(EvidenceKind::Network, "network", "n1", now))
                .map(|signal| signal.kind),
            Some(RuntimeSignalKind::Network)
        );
        assert_eq!(
            runtime_signal(&evidence(EvidenceKind::Semantic, "dom_mutation", "s1", now))
                .map(|signal| signal.kind),
            Some(RuntimeSignalKind::DomMutation)
        );
        assert!(
            runtime_signal(&evidence(EvidenceKind::Console, "console", "c1", now)).is_none()
        );
        assert!(
            runtime_signal(&evidence(EvidenceKind::Interaction, "focus", "f1", now)).is_none()
        );
    }
}
