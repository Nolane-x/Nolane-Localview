use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    MutationCase, MutationOperator, MutationOutcome, MutationSafetyPolicy, MutationVerdict,
    evaluate_mutation,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyntheticMutationState {
    pub target_present: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
    pub accessible_name: Option<String>,
    pub tab_order_valid: bool,
    pub handler_enabled: bool,
    pub feedback_delay_ms: u64,
    pub loopback_http_status: Option<u16>,
    pub loopback_timeout_ms: Option<u64>,
    pub content: String,
    pub visual_tokens: BTreeMap<String, Value>,
}

impl Default for SyntheticMutationState {
    fn default() -> Self {
        Self {
            target_present: true,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
            visible: true,
            accessible_name: Some("action".into()),
            tab_order_valid: true,
            handler_enabled: true,
            feedback_delay_ms: 0,
            loopback_http_status: None,
            loopback_timeout_ms: None,
            content: "content".into(),
            visual_tokens: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationExecutionPolicy {
    pub isolated_state: bool,
    pub loopback_network_fault_authority: bool,
    pub external_side_effects_allowed: bool,
    pub safety: MutationSafetyPolicy,
}

impl Default for MutationExecutionPolicy {
    fn default() -> Self {
        Self {
            isolated_state: true,
            loopback_network_fault_authority: false,
            external_side_effects_allowed: false,
            safety: MutationSafetyPolicy {
                allow_network_failure: false,
                allow_source_overlay: true,
                allow_external_side_effects: false,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationExecutionEvidence {
    pub mutation_id: uuid::Uuid,
    pub safety_checked: bool,
    pub applied_in_isolation: bool,
    pub external_side_effects: bool,
    pub before_digest: String,
    pub after_digest: Option<String>,
    pub triggered_detectors: BTreeSet<String>,
    pub reason: String,
    pub evidence_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MutationChallengeResult {
    pub outcome: MutationOutcome,
    pub execution: MutationExecutionEvidence,
}

pub fn execute_mutation_challenge<F>(
    case: &MutationCase,
    baseline: &SyntheticMutationState,
    policy: &MutationExecutionPolicy,
    detector: F,
) -> MutationChallengeResult
where
    F: FnOnce(&SyntheticMutationState, &SyntheticMutationState) -> BTreeSet<String>,
{
    let before_digest = state_digest(baseline);
    let safety = wave9_policy_allows(case, policy);
    if let Err(reason) = safety {
        let evidence_id =
            execution_evidence_id(case, &before_digest, None, &BTreeSet::new(), &reason);
        let outcome = MutationOutcome {
            mutation_id: case.id,
            verdict: if case.expected_detectors.is_empty() {
                MutationVerdict::Invalid
            } else {
                MutationVerdict::SkippedUnsafe
            },
            triggered_detectors: BTreeSet::new(),
            evidence_ids: vec![evidence_id.clone()],
        };
        return MutationChallengeResult {
            outcome,
            execution: MutationExecutionEvidence {
                mutation_id: case.id,
                safety_checked: true,
                applied_in_isolation: false,
                external_side_effects: false,
                before_digest,
                after_digest: None,
                triggered_detectors: BTreeSet::new(),
                reason,
                evidence_id,
            },
        };
    }

    let mut candidate = baseline.clone();
    let applied = apply_operator(&mut candidate, &case.operator);
    if let Err(reason) = applied {
        let evidence_id =
            execution_evidence_id(case, &before_digest, None, &BTreeSet::new(), &reason);
        return MutationChallengeResult {
            outcome: MutationOutcome {
                mutation_id: case.id,
                verdict: MutationVerdict::Invalid,
                triggered_detectors: BTreeSet::new(),
                evidence_ids: vec![evidence_id.clone()],
            },
            execution: MutationExecutionEvidence {
                mutation_id: case.id,
                safety_checked: true,
                applied_in_isolation: false,
                external_side_effects: false,
                before_digest,
                after_digest: None,
                triggered_detectors: BTreeSet::new(),
                reason,
                evidence_id,
            },
        };
    }

    let after_digest = state_digest(&candidate);
    let triggered = detector(baseline, &candidate);
    let reason = if triggered.is_empty() {
        "isolated mutation executed; no detector triggered".to_string()
    } else {
        format!(
            "isolated mutation executed; {} detector(s) triggered",
            triggered.len()
        )
    };
    let evidence_id = execution_evidence_id(
        case,
        &before_digest,
        Some(&after_digest),
        &triggered,
        &reason,
    );
    let outcome = evaluate_mutation(case, triggered.clone(), vec![evidence_id.clone()]);

    MutationChallengeResult {
        outcome,
        execution: MutationExecutionEvidence {
            mutation_id: case.id,
            safety_checked: true,
            applied_in_isolation: true,
            external_side_effects: false,
            before_digest,
            after_digest: Some(after_digest),
            triggered_detectors: triggered,
            reason,
            evidence_id,
        },
    }
}

pub fn run_mutation_suite<F>(
    cases: &[MutationCase],
    baseline: &SyntheticMutationState,
    policy: &MutationExecutionPolicy,
    mut detector: F,
) -> Vec<MutationChallengeResult>
where
    F: FnMut(&MutationCase, &SyntheticMutationState, &SyntheticMutationState) -> BTreeSet<String>,
{
    cases
        .iter()
        .map(|case| {
            execute_mutation_challenge(case, baseline, policy, |before, after| {
                detector(case, before, after)
            })
        })
        .collect()
}

fn wave9_policy_allows(
    case: &MutationCase,
    policy: &MutationExecutionPolicy,
) -> Result<(), String> {
    if case.expected_detectors.is_empty() {
        return Err("mutation has no expected detector and is invalid".into());
    }
    if !case.safe_to_run {
        return Err("mutation case is explicitly unsafe".into());
    }
    if !policy.isolated_state {
        return Err("Wave 9 mutations require isolated synthetic or shadow state".into());
    }
    if policy.external_side_effects_allowed || policy.safety.allow_external_side_effects {
        return Err("Wave 9 mutation execution forbids external side effects".into());
    }

    match case.operator {
        MutationOperator::ForceHttpStatus { .. } | MutationOperator::ForceTimeout { .. } => {
            if !policy.safety.allow_network_failure {
                return Err("network failure mutation is disabled by safety policy".into());
            }
            if !policy.loopback_network_fault_authority {
                return Err(
                    "network failure mutation requires proven loopback fault authority".into(),
                );
            }
        }
        _ if !policy.safety.allow_source_overlay => {
            return Err("source/semantic overlay mutation is disabled by safety policy".into());
        }
        _ => {}
    }
    Ok(())
}

fn apply_operator(
    state: &mut SyntheticMutationState,
    operator: &MutationOperator,
) -> Result<(), String> {
    match operator {
        MutationOperator::Shift { dx, dy } => {
            if !dx.is_finite() || !dy.is_finite() {
                return Err("layout shift is non-finite".into());
            }
            state.x += dx;
            state.y += dy;
        }
        MutationOperator::Resize {
            width_factor,
            height_factor,
        } => {
            if !width_factor.is_finite()
                || !height_factor.is_finite()
                || *width_factor <= 0.0
                || *height_factor <= 0.0
            {
                return Err("resize factors must be finite and positive".into());
            }
            state.width *= width_factor;
            state.height *= height_factor;
        }
        MutationOperator::Hide => state.visible = false,
        MutationOperator::RemoveAccessibleName => state.accessible_name = None,
        MutationOperator::BreakTabOrder => state.tab_order_valid = false,
        MutationOperator::DisableHandler => state.handler_enabled = false,
        MutationOperator::DelayFeedback { milliseconds } => {
            state.feedback_delay_ms = *milliseconds;
        }
        MutationOperator::ForceHttpStatus { status } => {
            if !(100..=599).contains(status) {
                return Err("synthetic HTTP status is outside the valid range".into());
            }
            state.loopback_http_status = Some(*status);
        }
        MutationOperator::ForceTimeout { milliseconds } => {
            if *milliseconds == 0 || *milliseconds > 60_000 {
                return Err("synthetic timeout is outside the bounded range".into());
            }
            state.loopback_timeout_ms = Some(*milliseconds);
        }
        MutationOperator::ReplaceContent { value } => {
            if value.len() > 64 * 1024 {
                return Err("replacement content exceeds synthetic bound".into());
            }
            state.content = value.clone();
        }
        MutationOperator::TokenOverride { token, value } => {
            if token.is_empty() || token.len() > 256 {
                return Err("visual token key is outside the bounded range".into());
            }
            state.visual_tokens.insert(token.clone(), value.clone());
        }
    }
    Ok(())
}

fn state_digest(state: &SyntheticMutationState) -> String {
    let bytes = serde_json::to_vec(state).unwrap_or_default();
    format!("sha256:{}", hex_lower(&Sha256::digest(bytes)))
}

fn execution_evidence_id(
    case: &MutationCase,
    before: &str,
    after: Option<&str>,
    triggered: &BTreeSet<String>,
    reason: &str,
) -> String {
    let bytes =
        serde_json::to_vec(&(case.id, before, after, triggered, reason)).unwrap_or_default();
    format!("mutation:{}", hex_lower(&Sha256::digest(bytes)))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MutationOperator;
    use uuid::Uuid;

    fn case(operator: MutationOperator, expected: &[&str]) -> MutationCase {
        MutationCase {
            id: Uuid::new_v4(),
            target: "@e1".into(),
            operator,
            expected_detectors: expected.iter().map(|item| (*item).into()).collect(),
            safe_to_run: true,
            relevance: 1.0,
        }
    }

    #[test]
    fn isolated_mutation_is_killed_when_expected_detector_observes_it() {
        let case = case(MutationOperator::RemoveAccessibleName, &["a11y-name"]);
        let result = execute_mutation_challenge(
            &case,
            &SyntheticMutationState::default(),
            &MutationExecutionPolicy::default(),
            |_, after| {
                if after.accessible_name.is_none() {
                    BTreeSet::from(["a11y-name".into()])
                } else {
                    BTreeSet::new()
                }
            },
        );
        assert_eq!(result.outcome.verdict, MutationVerdict::Killed);
        assert!(result.execution.applied_in_isolation);
        assert!(!result.execution.external_side_effects);
    }

    #[test]
    fn surviving_mutation_is_preserved_as_survived() {
        let case = case(MutationOperator::DisableHandler, &["dead-click"]);
        let result = execute_mutation_challenge(
            &case,
            &SyntheticMutationState::default(),
            &MutationExecutionPolicy::default(),
            |_, _| BTreeSet::new(),
        );
        assert_eq!(result.outcome.verdict, MutationVerdict::Survived);
        assert_eq!(result.execution.triggered_detectors.len(), 0);
    }

    #[test]
    fn unsafe_or_external_side_effect_policy_is_skipped_not_score_hidden() {
        let mut unsafe_case = case(MutationOperator::Hide, &["layout"]);
        unsafe_case.safe_to_run = false;
        let result = execute_mutation_challenge(
            &unsafe_case,
            &SyntheticMutationState::default(),
            &MutationExecutionPolicy::default(),
            |_, _| panic!("unsafe mutation must not execute detectors"),
        );
        assert_eq!(result.outcome.verdict, MutationVerdict::SkippedUnsafe);

        let external = MutationExecutionPolicy {
            external_side_effects_allowed: true,
            ..Default::default()
        };
        let result = execute_mutation_challenge(
            &case(MutationOperator::Hide, &["layout"]),
            &SyntheticMutationState::default(),
            &external,
            |_, _| panic!("external mutation must not execute"),
        );
        assert_eq!(result.outcome.verdict, MutationVerdict::SkippedUnsafe);
    }

    #[test]
    fn network_mutation_requires_loopback_fault_authority() {
        let network_case = case(
            MutationOperator::ForceHttpStatus { status: 503 },
            &["network-failure"],
        );
        let mut policy = MutationExecutionPolicy::default();
        policy.safety.allow_network_failure = true;
        let denied = execute_mutation_challenge(
            &network_case,
            &SyntheticMutationState::default(),
            &policy,
            |_, _| panic!("network mutation must not execute without loopback authority"),
        );
        assert_eq!(denied.outcome.verdict, MutationVerdict::SkippedUnsafe);

        policy.loopback_network_fault_authority = true;
        let allowed = execute_mutation_challenge(
            &network_case,
            &SyntheticMutationState::default(),
            &policy,
            |_, after| {
                if after.loopback_http_status == Some(503) {
                    BTreeSet::from(["network-failure".into()])
                } else {
                    BTreeSet::new()
                }
            },
        );
        assert_eq!(allowed.outcome.verdict, MutationVerdict::Killed);
        assert!(!allowed.execution.external_side_effects);
    }
}
