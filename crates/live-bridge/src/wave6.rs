use localview_flow::{
    InteractionActionKind, ReplayStep, SafetyClass, StateIdentity,
};
use localview_protocol::SessionId;
use serde::{Deserialize, Serialize};

use crate::{BridgeAction, BridgeActionKind, LiveBridge};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Wave6ReplayAdmissionError {
    UnsafeAction,
    StableRefInvalid,
    PreStateMismatch,
    UnsupportedReplayPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave6ReplayAdmission {
    pub step_index: usize,
    pub action_id: uuid::Uuid,
    pub target: String,
    pub before_state: StateIdentity,
}

/// Queue exactly one bounded Wave 6 replay step through the existing public
/// deterministic action queue.
///
/// This method does not repair stale references, guess selectors, or authorize
/// unknown/destructive actions. The caller must provide the fresh state observed
/// immediately before the step. Post-state verification remains a separate
/// fresh observation fed into localview-flow's replay receipt.
impl LiveBridge {
    pub async fn enqueue_wave6_replay_step(
        &self,
        session_id: SessionId,
        step: &ReplayStep,
        observed_before: &StateIdentity,
        safety: SafetyClass,
        stable_ref_valid: bool,
    ) -> Result<(BridgeAction, Wave6ReplayAdmission), Wave6ReplayAdmissionError> {
        if !safety.may_probe() {
            return Err(Wave6ReplayAdmissionError::UnsafeAction);
        }
        if !stable_ref_valid {
            return Err(Wave6ReplayAdmissionError::StableRefInvalid);
        }
        if !step.expected_before.compatible_with(observed_before) {
            return Err(Wave6ReplayAdmissionError::PreStateMismatch);
        }

        let action = match step.action {
            InteractionActionKind::Click => BridgeActionKind::Click,
            InteractionActionKind::Focus => BridgeActionKind::Focus,
            InteractionActionKind::Tab => BridgeActionKind::Key {
                key: "Tab".into(),
                modifiers: Vec::new(),
            },
            InteractionActionKind::ShiftTab => BridgeActionKind::Key {
                key: "Tab".into(),
                modifiers: vec!["Shift".into()],
            },
            InteractionActionKind::Key | InteractionActionKind::Scroll => {
                return Err(Wave6ReplayAdmissionError::UnsupportedReplayPayload);
            }
        };

        let queued = self
            .enqueue_action(session_id, Some(step.target.clone()), action)
            .await;
        let receipt = Wave6ReplayAdmission {
            step_index: step.index,
            action_id: queued.id,
            target: step.target.clone(),
            before_state: observed_before.clone(),
        };
        Ok((queued, receipt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(fingerprint: &str) -> StateIdentity {
        StateIdentity {
            route: "http://127.0.0.1:3000/".into(),
            document_generation: 7,
            semantic_fingerprint: fingerprint.into(),
            viewport: Some((800, 600)),
        }
    }

    fn step(action: InteractionActionKind) -> ReplayStep {
        ReplayStep {
            index: 0,
            action,
            target: "@eabc".into(),
            expected_before: state("before"),
            expected_after: state("after"),
            evidence_refs: vec![],
        }
    }

    #[tokio::test]
    async fn safe_click_replay_reuses_existing_action_queue() {
        let bridge = LiveBridge::new(32, 8);
        let session = uuid::Uuid::new_v4();
        let (queued, receipt) = bridge
            .enqueue_wave6_replay_step(
                session,
                &step(InteractionActionKind::Click),
                &state("before"),
                SafetyClass::ExplicitlySafe,
                true,
            )
            .await
            .unwrap();
        assert!(matches!(queued.action, BridgeActionKind::Click));
        assert_eq!(receipt.action_id, queued.id);
        let taken = bridge.take_actions(session, 8).await;
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].id, queued.id);
    }

    #[tokio::test]
    async fn replay_refuses_state_drift_and_unknown_safety() {
        let bridge = LiveBridge::new(32, 8);
        let session = uuid::Uuid::new_v4();
        let drift = bridge
            .enqueue_wave6_replay_step(
                session,
                &step(InteractionActionKind::Click),
                &state("different"),
                SafetyClass::ExplicitlySafe,
                true,
            )
            .await
            .unwrap_err();
        assert_eq!(drift, Wave6ReplayAdmissionError::PreStateMismatch);

        let unsafe_action = bridge
            .enqueue_wave6_replay_step(
                session,
                &step(InteractionActionKind::Click),
                &state("before"),
                SafetyClass::Unknown,
                true,
            )
            .await
            .unwrap_err();
        assert_eq!(unsafe_action, Wave6ReplayAdmissionError::UnsafeAction);
        assert!(bridge.take_actions(session, 8).await.is_empty());
    }

    #[tokio::test]
    async fn generic_key_or_scroll_replay_never_invents_missing_payload() {
        let bridge = LiveBridge::new(32, 8);
        let session = uuid::Uuid::new_v4();
        let error = bridge
            .enqueue_wave6_replay_step(
                session,
                &step(InteractionActionKind::Key),
                &state("before"),
                SafetyClass::ExplicitlySafe,
                true,
            )
            .await
            .unwrap_err();
        assert_eq!(error, Wave6ReplayAdmissionError::UnsupportedReplayPayload);
    }
}
