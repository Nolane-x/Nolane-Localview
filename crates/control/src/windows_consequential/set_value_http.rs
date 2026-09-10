use std::{collections::HashMap, fmt};

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use localview_live_bridge::{SetValueCommitmentKey, SetValueMode, SetValuePayloadRef};
use localview_postcondition_contracts::{
    PayloadEqualityModeV1, PayloadEqualityPostconditionContractV1,
};
use localview_protocol::{ProviderElementRef, SessionId};
use localview_windows_uia_provider::MAX_SET_VALUE_UTF8_BYTES;
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::*;

#[derive(Clone)]
pub(super) struct WindowsSetValuePayloadAuthority {
    commitment_key: Arc<SetValueCommitmentKey>,
    pending: Arc<Mutex<HashMap<Uuid, PendingWindowsSetValuePayload>>>,
}

impl WindowsSetValuePayloadAuthority {
    pub(super) fn new() -> Result<Self, String> {
        let commitment_key =
            SetValueCommitmentKey::generate().map_err(|error| error.to_string())?;
        Ok(Self {
            commitment_key: Arc::new(commitment_key),
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Task 8 control-owned durable commitment bridge is armed before the server-owned planning route consumes it"
        )
    )]
    async fn persist_binding(
        &self,
        journal: &localview_live_bridge::ConsequentialJournal,
        queued: &localview_live_bridge::CanonicalQueuedAction,
        payload: &ProcessLocalSetValuePayload,
    ) -> Result<
        localview_live_bridge::DurableSetValuePayloadBinding,
        localview_live_bridge::ConsequentialJournalError,
    > {
        journal
            .record_set_value_payload_binding(
                queued,
                self.commitment_key.as_ref(),
                payload.payload_ref,
                payload.mode,
                payload.utf8_bytes(),
            )
            .await
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Task 8 authority-owned staging is introduced before the server-owned route consumes it"
        )
    )]
    async fn stage(
        &self,
        action_id: Uuid,
        candidate: PendingWindowsSetValuePayload,
    ) -> Result<(), &'static str> {
        use std::collections::hash_map::Entry;

        match self.pending.lock().await.entry(action_id) {
            Entry::Vacant(entry) => {
                entry.insert(candidate);
                Ok(())
            }
            Entry::Occupied(_) => Err("SetValue payload authority already exists for action"),
        }
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Task 8 authority-owned peek is introduced before exact-confirmation route wiring"
        )
    )]
    async fn peek(
        &self,
        session_id: SessionId,
        action_id: Uuid,
        confirmation_ref: Uuid,
    ) -> bool {
        let pending = self.pending.lock().await;
        peek_pending_set_value_payload(&pending, session_id, action_id, confirmation_ref)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Task 8 authority-owned consume is introduced before exact-confirmation route wiring"
        )
    )]
    async fn consume(
        &self,
        session_id: SessionId,
        action_id: Uuid,
        confirmation_ref: Uuid,
    ) -> Option<PendingWindowsSetValuePayload> {
        let mut pending = self.pending.lock().await;
        consume_pending_set_value_payload(&mut pending, session_id, action_id, confirmation_ref)
    }

    pub(super) async fn release_session(&self, session_id: SessionId) {
        self.pending
            .lock()
            .await
            .retain(|_, candidate| candidate.session_id != session_id);
    }
}

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

struct PendingWindowsSetValuePayload {
    session_id: SessionId,
    confirmation_ref: Uuid,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Task 8 payload remains process-local but is not read until Stage 3 exact-confirmation dispatch wiring"
        )
    )]
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

struct PreparedServerOwnedSetValuePayload {
    payload: ProcessLocalSetValuePayload,
    expected_postcondition_contract_ref: String,
}

fn prepare_server_owned_set_value_payload(
    mode: SetValuePlanMode,
    payload_ref: SetValuePayloadRef,
) -> Result<PreparedServerOwnedSetValuePayload, String> {
    let (payload_mode, contract_mode, utf8) = match mode {
        SetValuePlanMode::ReplaceValue(value) => (
            SetValueMode::ReplaceValue,
            PayloadEqualityModeV1::ReplaceValue,
            value.into_bytes(),
        ),
        SetValuePlanMode::ClearValue => (
            SetValueMode::ClearValue,
            PayloadEqualityModeV1::ClearValue,
            Vec::new(),
        ),
    };
    let payload = ProcessLocalSetValuePayload::new(payload_ref, payload_mode, utf8)
        .map_err(str::to_owned)?;
    let expected_postcondition_contract_ref = PayloadEqualityPostconditionContractV1 {
        mode: contract_mode,
        payload_ref: payload_ref.0.to_string(),
    }
    .to_contract_ref()
    .map_err(|error| error.to_string())?;

    Ok(PreparedServerOwnedSetValuePayload {
        payload,
        expected_postcondition_contract_ref,
    })
}

pub(super) async fn plan_windows_consequential_set_value(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Path(session_id): Path<SessionId>,
    body: Bytes,
) -> axum::response::Response {
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

    let Some(_runtime) = windows_observe_runtime_for_sessions(&state.sessions) else {
        return unavailable("Windows UIA runtime is unavailable");
    };
    let Some(_control) = windows_consequential_control_for_sessions(&state.sessions) else {
        return unavailable("durable consequential control journal is unavailable");
    };

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
    use std::{collections::HashMap, path::PathBuf};

    use localview_live_bridge::{
        verify_set_value_payload_binding, ActionEnvelopeMetadata, ActionIdempotencyClass,
        ActionRiskClass, BridgeActionKind, CanonicalActionOperation, ConsequentialJournal,
        LiveBridge, ProviderObservationBinding,
    };
    use localview_protocol::{
        EventContinuityState, PrincipalRef, ProviderIncarnationRef, TargetIncarnationRef,
    };
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

    fn journal_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("localview-{label}-{}.jsonl", Uuid::new_v4()))
    }

    async fn admitted_set_value_action() -> localview_live_bridge::CanonicalQueuedAction {
        let bridge = LiveBridge::new(32, 8);
        let session_id: SessionId = Uuid::from_u128(0x8a30);
        let provider = ProviderIncarnationRef::from("provider:windows-uia:task8-control-binding");
        let target = TargetIncarnationRef::from("target:windows:task8-control-binding");
        bridge
            .bind_provider_observation(ProviderObservationBinding {
                session_id,
                generation: 1,
                provider_incarnation_ref: provider.clone(),
                target_incarnation_ref: target.clone(),
                initial_continuity: EventContinuityState::OrderingOpaque,
                sequence_baseline: Some(0),
            })
            .await
            .unwrap();

        bridge
            .bind_direct_canonical_action(
                session_id,
                None,
                BridgeActionKind::TypeText {
                    text: String::new(),
                    clear_first: false,
                },
                ActionEnvelopeMetadata {
                    decision_principal_ref: PrincipalRef::from("principal:task8:decision"),
                    acting_principal_ref: PrincipalRef::from("principal:task8:acting"),
                    authorization_revision: "authorization:task8:v1".into(),
                    precondition_snapshot_cut_ref: "cut:task8:1".into(),
                    provider_incarnation_ref: provider,
                    target_incarnation_ref: target,
                    risk_class: ActionRiskClass::DestructiveOrIrreversible,
                    idempotency_class: ActionIdempotencyClass::Irreversible,
                    expected_postcondition_contract_refs: vec![
                        "postcondition:task8:set-value-binding".into(),
                    ],
                },
            )
            .await
            .unwrap()
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

    #[tokio::test]
    async fn payload_authority_refuses_duplicate_action_stage_and_consumes_exactly_once() {
        let authority = WindowsSetValuePayloadAuthority::new().expect("process-local authority");
        let session_id = Uuid::from_u128(0x8a20);
        let action_id = Uuid::from_u128(0x8a21);
        let confirmation_ref = Uuid::from_u128(0x8a22);

        authority
            .stage(action_id, pending_payload(session_id, confirmation_ref))
            .await
            .expect("first stage must reserve exact action authority");
        assert!(
            authority
                .peek(session_id, action_id, confirmation_ref)
                .await,
            "staged payload must be visible only through exact session/action/confirmation metadata"
        );

        let duplicate = authority
            .stage(action_id, pending_payload(session_id, confirmation_ref))
            .await
            .expect_err("duplicate action id must not replace live plaintext authority");
        assert_eq!(duplicate, "SetValue payload authority already exists for action");

        let consumed = authority
            .consume(session_id, action_id, confirmation_ref)
            .await
            .expect("exact metadata consumes the staged payload");
        assert_eq!(consumed.payload.utf8_bytes(), SENTINEL.as_bytes());
        assert!(
            authority
                .consume(session_id, action_id, confirmation_ref)
                .await
                .is_none(),
            "SetValue payload authority must be one-shot"
        );
    }

    #[tokio::test]
    async fn payload_authority_persists_exact_opaque_binding_with_process_key() {
        let authority = WindowsSetValuePayloadAuthority::new().expect("process-local authority");
        let queued = admitted_set_value_action().await;
        let path = journal_path("task8-control-set-value-binding");
        let journal = ConsequentialJournal::open(&path).await.unwrap();
        journal
            .record_intent_admitted(queued.envelope.clone())
            .await
            .unwrap();
        journal
            .record_intent_operation_bound_explicit(&queued, CanonicalActionOperation::SetValue)
            .await
            .unwrap();
        let payload = ProcessLocalSetValuePayload::new(
            SetValuePayloadRef(Uuid::from_u128(0x8a31)),
            SetValueMode::ReplaceValue,
            SENTINEL.as_bytes().to_vec(),
        )
        .unwrap();

        let binding = authority
            .persist_binding(&journal, &queued, &payload)
            .await
            .expect("control-owned authority must persist the exact opaque payload commitment");

        assert_eq!(binding.action_id, queued.action.id);
        assert_eq!(binding.payload_ref, payload.payload_ref);
        assert_eq!(binding.mode, payload.mode);
        assert_eq!(binding.payload_utf8_len, SENTINEL.len() as u64);
        assert!(verify_set_value_payload_binding(
            &authority.commitment_key,
            &binding,
            SENTINEL.as_bytes(),
        )
        .is_ok());
        let encoded = serde_json::to_vec(&binding).unwrap();
        assert!(!encoded.windows(SENTINEL.len()).any(|window| window == SENTINEL.as_bytes()));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn server_owned_set_value_payload_contract_is_opaque_and_exact() {
        let payload_ref = SetValuePayloadRef(Uuid::from_u128(0x8a40));
        let prepared = prepare_server_owned_set_value_payload(
            SetValuePlanMode::ReplaceValue(SENTINEL.to_owned()),
            payload_ref,
        )
        .expect("valid SetValue payload preparation");

        assert_eq!(prepared.payload.payload_ref, payload_ref);
        assert_eq!(prepared.payload.mode, SetValueMode::ReplaceValue);
        assert_eq!(prepared.payload.utf8_bytes(), SENTINEL.as_bytes());
        assert_eq!(
            prepared.expected_postcondition_contract_ref,
            format!(
                "lvpc:payload-equality:v1:{{\"mode\":\"replace_value\",\"payload_ref\":\"{}\"}}",
                payload_ref.0
            )
        );
        assert!(!prepared.expected_postcondition_contract_ref.contains(SENTINEL));
    }
}
