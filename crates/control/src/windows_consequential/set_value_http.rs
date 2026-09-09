use std::{collections::HashMap, fmt};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use localview_live_bridge::{SetValueMode, SetValuePayloadRef};
use localview_protocol::{ProviderElementRef, SessionId};
use localview_windows_uia_provider::MAX_SET_VALUE_UTF8_BYTES;
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::*;

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Task 8 Stage 2 payload authority is intentionally introduced before Stage 3 server-owned route wiring"
    )
)]
struct ProcessLocalSetValuePayload {
    payload_ref: SetValuePayloadRef,
    mode: SetValueMode,
    utf8: Zeroizing<Vec<u8>>,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Task 8 Stage 2 payload authority is intentionally introduced before Stage 3 server-owned route wiring"
    )
)]
impl ProcessLocalSetValuePayload {
    fn new(
        payload_ref: SetValuePayloadRef,
        mode: SetValueMode,
        utf8: Vec<u8>,
    ) -> Result<Self, &'static str> {
        if utf8.len() > MAX_SET_VALUE_UTF8_BYTES {
            return Err("SetValue payload exceeds 16 KiB UTF-8 limit");
        }
        if utf8.contains(&0) {
            return Err("SetValue payload contains U+0000");
        }
        if matches!(mode, SetValueMode::ClearValue) && !utf8.is_empty() {
            return Err("clear_value payload must be empty");
        }

        Ok(Self {
            payload_ref,
            mode,
            utf8: Zeroizing::new(utf8),
        })
    }

    fn utf8_bytes(&self) -> &[u8] {
        self.utf8.as_slice()
    }

    fn utf8_len(&self) -> usize {
        self.utf8.len()
    }
}

impl fmt::Debug for ProcessLocalSetValuePayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessLocalSetValuePayload")
            .field("payload_ref", &self.payload_ref)
            .field("mode", &self.mode)
            .field("utf8_len", &self.utf8_len())
            .finish()
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Task 8 Stage 2 pending payload authority is intentionally introduced before Stage 3 confirmation wiring"
    )
)]
struct PendingWindowsSetValuePayload {
    session_id: SessionId,
    confirmation_ref: Uuid,
    payload: ProcessLocalSetValuePayload,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Task 8 Stage 2 exact-confirmation peek is intentionally introduced before Stage 3 confirmation wiring"
    )
)]
fn peek_pending_set_value_payload(
    pending: &HashMap<Uuid, PendingWindowsSetValuePayload>,
    session_id: SessionId,
    action_id: Uuid,
    confirmation_ref: Uuid,
) -> bool {
    pending.get(&action_id).is_some_and(|candidate| {
        candidate.session_id == session_id && candidate.confirmation_ref == confirmation_ref
    })
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Task 8 Stage 2 one-shot consume is intentionally introduced before Stage 3 confirmation wiring"
    )
)]
fn consume_pending_set_value_payload(
    pending: &mut HashMap<Uuid, PendingWindowsSetValuePayload>,
    session_id: SessionId,
    action_id: Uuid,
    confirmation_ref: Uuid,
) -> Option<PendingWindowsSetValuePayload> {
    if !peek_pending_set_value_payload(pending, session_id, action_id, confirmation_ref) {
        return None;
    }
    pending.remove(&action_id)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum WindowsSetValuePlanRequest {
    ReplaceValue {
        element_ref: ProviderElementRef,
        value: String,
    },
    ClearValue {
        element_ref: ProviderElementRef,
    },
}

impl WindowsSetValuePlanRequest {
    fn validate(self) -> Result<(ProviderElementRef, SetValuePlanMode), &'static str> {
        match self {
            Self::ReplaceValue { element_ref, value } => {
                if value.len() > MAX_SET_VALUE_UTF8_BYTES {
                    return Err("replace_value payload exceeds 16 KiB UTF-8 limit");
                }
                if value.as_bytes().contains(&0) {
                    return Err("replace_value payload contains U+0000");
                }
                Ok((element_ref, SetValuePlanMode::ReplaceValue(value)))
            }
            Self::ClearValue { element_ref } => Ok((element_ref, SetValuePlanMode::ClearValue)),
        }
    }
}

enum SetValuePlanMode {
    ReplaceValue(String),
    ClearValue,
}

impl SetValuePlanMode {
    fn payload_len(&self) -> usize {
        match self {
            Self::ReplaceValue(value) => value.len(),
            Self::ClearValue => 0,
        }
    }
}

pub(super) async fn plan_windows_consequential_set_value(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(session_id): Path<SessionId>,
    body: Bytes,
) -> axum::response::Response {
    // Body bytes are intentionally not deserialized until bearer and session
    // authority have been checked. This keeps malformed or hostile payloads out
    // of the consequential planning domain entirely.
    if !authorized(&headers, &state) {
        return denied();
    }
    if state.sessions.get(session_id).await.is_none() {
        return session_not_found();
    }

    let request = match serde_json::from_slice::<WindowsSetValuePlanRequest>(&body) {
        Ok(request) => request,
        Err(_) => return invalid_set_value_request("SetValue plan request shape is invalid"),
    };
    let (_element_ref, mode) = match request.validate() {
        Ok(validated) => validated,
        Err(message) => return invalid_set_value_request(message),
    };

    // Do not allocate action ids, confirmation capabilities, HMAC material or
    // durable intent before the exact runtime/control dependencies exist.
    let Some(_runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable("Windows UIA runtime is unavailable");
    };
    let Some(_control) = windows_consequential_control_for_sessions(&state.sessions) else {
        return unavailable("durable consequential control journal is unavailable");
    };

    // The next Task-8 GREEN slice replaces this fail-closed boundary with the
    // server-owned fresh-evidence/HMAC/pending-payload admission protocol.
    let _payload_len = mode.payload_len();
    unavailable("Windows UIA SetValue server-owned planning is not yet armed")
}

fn invalid_set_value_request(message: &'static str) -> axum::response::Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({
            "error": "invalid_windows_set_value_plan_request",
            "message": message,
            "dispatch_performed": false,
            "confirmation_created": false,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use uuid::Uuid;

    use super::*;

    const SENTINEL: &str = "localview-task8-process-local-secret-9124f98d";

    fn pending_payload(
        session_id: SessionId,
        confirmation_ref: Uuid,
    ) -> PendingWindowsSetValuePayload {
        PendingWindowsSetValuePayload {
            session_id,
            confirmation_ref,
            payload: ProcessLocalSetValuePayload::new(
                SetValuePayloadRef(Uuid::from_u128(0x8a01)),
                SetValueMode::ReplaceValue,
                SENTINEL.as_bytes().to_vec(),
            )
            .expect("valid bounded process-local payload"),
        }
    }

    #[test]
    fn process_local_payload_debug_is_metadata_only() {
        let payload = ProcessLocalSetValuePayload::new(
            SetValuePayloadRef(Uuid::from_u128(0x8a02)),
            SetValueMode::ReplaceValue,
            SENTINEL.as_bytes().to_vec(),
        )
        .expect("valid bounded process-local payload");

        assert_eq!(payload.utf8_bytes(), SENTINEL.as_bytes());
        assert_eq!(payload.utf8_len(), SENTINEL.len());
        let debug = format!("{payload:?}");
        assert!(!debug.contains(SENTINEL));
        assert!(debug.contains("utf8_len"));
    }

    #[test]
    fn wrong_confirmation_keeps_set_value_payload_and_exact_confirmation_moves_once() {
        let session_id = Uuid::from_u128(0x8a10);
        let action_id = Uuid::from_u128(0x8a11);
        let confirmation_ref = Uuid::from_u128(0x8a12);
        let wrong_confirmation = Uuid::from_u128(0xdead);
        let mut pending = HashMap::new();
        pending.insert(action_id, pending_payload(session_id, confirmation_ref));

        assert!(
            consume_pending_set_value_payload(
                &mut pending,
                session_id,
                action_id,
                wrong_confirmation,
            )
            .is_none(),
            "wrong confirmation must not consume process-local payload authority"
        );
        assert!(peek_pending_set_value_payload(
            &pending,
            session_id,
            action_id,
            confirmation_ref,
        ));

        let consumed = consume_pending_set_value_payload(
            &mut pending,
            session_id,
            action_id,
            confirmation_ref,
        )
        .expect("exact confirmation moves payload authority");
        assert_eq!(consumed.payload.utf8_bytes(), SENTINEL.as_bytes());
        assert!(
            consume_pending_set_value_payload(
                &mut pending,
                session_id,
                action_id,
                confirmation_ref,
            )
            .is_none(),
            "exact confirmation is one-shot"
        );
    }
}
