use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use localview_protocol::{ProviderElementRef, SessionId};
use localview_windows_uia_provider::MAX_SET_VALUE_UTF8_BYTES;
use serde::Deserialize;

use super::*;

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
