#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceBudget {
    pub memory_mb: u64,
    pub cpu_percent: f32,
    pub capture_storage_mb: u64,
    pub network_kb_per_minute: u64,
    pub chromium_instances: usize,
    pub concurrent_captures: usize,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            memory_mb: 256,
            cpu_percent: 10.0,
            capture_storage_mb: 512,
            network_kb_per_minute: 1024,
            chromium_instances: 1,
            concurrent_captures: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceSample {
    pub memory_mb: u64,
    pub cpu_percent: f32,
    pub capture_storage_mb: u64,
    pub network_kb_per_minute: u64,
    pub chromium_instances: usize,
    pub concurrent_captures: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeResourceSample {
    pub memory_mb: u64,
    pub cpu_percent: f32,
    pub capture_storage_mb: u64,
    pub network_kb_per_minute: u64,
}

impl Default for RuntimeResourceSample {
    fn default() -> Self {
        Self {
            memory_mb: 0,
            cpu_percent: 0.0,
            capture_storage_mb: 0,
            network_kb_per_minute: 0,
        }
    }
}

impl RuntimeResourceSample {
    pub fn is_valid(&self) -> bool {
        self.cpu_percent.is_finite() && self.cpu_percent >= 0.0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PressureLevel {
    Normal,
    Elevated,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DegradationAction {
    None,
    ReduceObservationRate,
    PreferSemanticOverVisual,
    ReduceCaptureResolution,
    EvictOldArtifacts,
    SuspendBackgroundResponsiveSweeps,
    BlockChromiumEscalation,
    SerializeCaptures,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GovernorDecision {
    pub pressure: PressureLevel,
    pub actions: Vec<DegradationAction>,
    pub reasons: Vec<String>,
}

pub fn evaluate(sample: &ResourceSample, budget: &ResourceBudget) -> GovernorDecision {
    let ratios = [
        ratio(sample.memory_mb as f32, budget.memory_mb as f32),
        ratio(sample.cpu_percent, budget.cpu_percent),
        ratio(
            sample.capture_storage_mb as f32,
            budget.capture_storage_mb as f32,
        ),
        ratio(
            sample.network_kb_per_minute as f32,
            budget.network_kb_per_minute as f32,
        ),
        ratio(
            sample.chromium_instances as f32,
            budget.chromium_instances.max(1) as f32,
        ),
        ratio(
            sample.concurrent_captures as f32,
            budget.concurrent_captures.max(1) as f32,
        ),
    ];
    let peak = ratios.into_iter().fold(0.0_f32, f32::max);
    let pressure = if peak < 0.75 {
        PressureLevel::Normal
    } else if peak < 1.0 {
        PressureLevel::Elevated
    } else if peak < 1.5 {
        PressureLevel::High
    } else {
        PressureLevel::Critical
    };
    let mut actions = Vec::new();
    let mut reasons = Vec::new();
    if sample.cpu_percent > budget.cpu_percent * 0.75 {
        actions.push(DegradationAction::ReduceObservationRate);
        reasons.push("CPU budget is under pressure".into());
    }
    if sample.memory_mb > budget.memory_mb {
        actions.push(DegradationAction::PreferSemanticOverVisual);
        reasons.push("memory budget exceeded".into());
    }
    if sample.capture_storage_mb > budget.capture_storage_mb * 3 / 4 {
        actions.push(DegradationAction::EvictOldArtifacts);
        reasons.push("capture store is approaching its bound".into());
    }
    if sample.concurrent_captures >= budget.concurrent_captures.max(1) {
        actions.push(DegradationAction::SerializeCaptures);
        reasons.push("capture concurrency budget reached".into());
    }
    if sample.chromium_instances >= budget.chromium_instances.max(1) {
        actions.push(DegradationAction::BlockChromiumEscalation);
        reasons.push("Chromium instance budget reached".into());
    }
    if pressure >= PressureLevel::High {
        actions.push(DegradationAction::SuspendBackgroundResponsiveSweeps);
    }
    if pressure == PressureLevel::Critical {
        actions.push(DegradationAction::ReduceCaptureResolution);
    }
    if actions.is_empty() {
        actions.push(DegradationAction::None);
    }
    actions.sort_by_key(|action| *action as u8);
    actions.dedup();
    GovernorDecision {
        pressure,
        actions,
        reasons,
    }
}

fn ratio(value: f32, limit: f32) -> f32 {
    if limit <= 0.0 {
        if value > 0.0 {
            f32::INFINITY
        } else {
            0.0
        }
    } else {
        value / limit
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkCandidate {
    pub id: String,
    pub priority: u16,
    pub estimated_cpu_ms: u64,
    pub estimated_memory_mb: u64,
    pub requires_chromium: bool,
}

pub fn schedule(
    candidates: &[WorkCandidate],
    available_cpu_ms: u64,
    available_memory_mb: u64,
    allow_chromium: bool,
) -> Vec<WorkCandidate> {
    let mut candidates = candidates.to_vec();
    candidates.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.estimated_cpu_ms.cmp(&right.estimated_cpu_ms))
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut cpu = 0u64;
    let mut memory = 0u64;
    let mut selected = Vec::new();
    for candidate in candidates {
        if candidate.requires_chromium && !allow_chromium {
            continue;
        }
        if cpu.saturating_add(candidate.estimated_cpu_ms) > available_cpu_ms
            || memory.saturating_add(candidate.estimated_memory_mb) > available_memory_mb
        {
            continue;
        }
        cpu += candidate.estimated_cpu_ms;
        memory += candidate.estimated_memory_mb;
        selected.push(candidate);
    }
    selected
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ResourceWorkKind {
    NativeVisualCapture,
    Chromium,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceAdmissionDenial {
    pub work_kind: ResourceWorkKind,
    pub decision: GovernorDecision,
}

type ReservationKey = (String, String);

#[derive(Debug)]
struct RuntimeGovernorState {
    budget: ResourceBudget,
    sample: RuntimeResourceSample,
    reservations: BTreeMap<ReservationKey, ResourceWorkKind>,
}

#[derive(Clone, Debug)]
pub struct RuntimeResourceGovernor {
    inner: Arc<Mutex<RuntimeGovernorState>>,
}

impl Default for RuntimeResourceGovernor {
    fn default() -> Self {
        Self::new(ResourceBudget::default())
    }
}

impl RuntimeResourceGovernor {
    pub fn new(budget: ResourceBudget) -> Self {
        Self {
            inner: Arc::new(Mutex::new(RuntimeGovernorState {
                budget,
                sample: RuntimeResourceSample::default(),
                reservations: BTreeMap::new(),
            })),
        }
    }

    pub fn update_sample(&self, sample: RuntimeResourceSample) -> bool {
        if !sample.is_valid() {
            return false;
        }
        lock(&self.inner).sample = sample;
        true
    }

    pub fn decision(&self) -> GovernorDecision {
        let state = lock(&self.inner);
        decision_for_state(&state)
    }

    pub fn check(&self, work_kind: ResourceWorkKind) -> Result<GovernorDecision, ResourceAdmissionDenial> {
        let state = lock(&self.inner);
        let decision = decision_for_state(&state);
        if denied_by_decision(work_kind, &decision) {
            Err(ResourceAdmissionDenial {
                work_kind,
                decision,
            })
        } else {
            Ok(decision)
        }
    }

    pub fn reserve(
        &self,
        session_id: impl Into<String>,
        request_id: impl Into<String>,
        work_kind: ResourceWorkKind,
    ) -> Result<ResourceReservation, ResourceAdmissionDenial> {
        let key = (session_id.into(), request_id.into());
        let mut state = lock(&self.inner);
        let decision = decision_for_state(&state);
        if denied_by_decision(work_kind, &decision) || state.reservations.contains_key(&key) {
            let mut denial_decision = decision;
            if state.reservations.contains_key(&key) {
                denial_decision
                    .reasons
                    .push("resource reservation already exists".into());
            }
            return Err(ResourceAdmissionDenial {
                work_kind,
                decision: denial_decision,
            });
        }
        state.reservations.insert(key.clone(), work_kind);
        drop(state);
        Ok(ResourceReservation {
            governor: self.clone(),
            key: Some(key),
        })
    }

    pub fn release_session(&self, session_id: &str) -> usize {
        let mut state = lock(&self.inner);
        let before = state.reservations.len();
        state
            .reservations
            .retain(|(reserved_session, _), _| reserved_session != session_id);
        before.saturating_sub(state.reservations.len())
    }

    fn release_key(&self, key: &ReservationKey) {
        lock(&self.inner).reservations.remove(key);
    }
}

#[must_use = "resource reservations must remain alive for the full admitted operation"]
#[derive(Debug)]
pub struct ResourceReservation {
    governor: RuntimeResourceGovernor,
    key: Option<ReservationKey>,
}

impl ResourceReservation {
    pub fn release(mut self) {
        if let Some(key) = self.key.take() {
            self.governor.release_key(&key);
        }
    }
}

impl Drop for ResourceReservation {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.governor.release_key(&key);
        }
    }
}

fn lock(inner: &Mutex<RuntimeGovernorState>) -> MutexGuard<'_, RuntimeGovernorState> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn decision_for_state(state: &RuntimeGovernorState) -> GovernorDecision {
    let mut chromium_instances = 0usize;
    let mut concurrent_captures = 0usize;
    for kind in state.reservations.values() {
        match kind {
            ResourceWorkKind::NativeVisualCapture => {
                concurrent_captures = concurrent_captures.saturating_add(1)
            }
            ResourceWorkKind::Chromium => {
                chromium_instances = chromium_instances.saturating_add(1)
            }
        }
    }
    evaluate(
        &ResourceSample {
            memory_mb: state.sample.memory_mb,
            cpu_percent: state.sample.cpu_percent,
            capture_storage_mb: state.sample.capture_storage_mb,
            network_kb_per_minute: state.sample.network_kb_per_minute,
            chromium_instances,
            concurrent_captures,
        },
        &state.budget,
    )
}

fn denied_by_decision(work_kind: ResourceWorkKind, decision: &GovernorDecision) -> bool {
    match work_kind {
        ResourceWorkKind::NativeVisualCapture => {
            decision.pressure == PressureLevel::Critical
                || decision
                    .actions
                    .contains(&DegradationAction::PreferSemanticOverVisual)
                || decision
                    .actions
                    .contains(&DegradationAction::SerializeCaptures)
        }
        ResourceWorkKind::Chromium => {
            decision.pressure >= PressureLevel::High
                || decision
                    .actions
                    .contains(&DegradationAction::BlockChromiumEscalation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_pressure_degrades_visual_work_before_correctness() {
        let budget = ResourceBudget::default();
        let sample = ResourceSample {
            memory_mb: 600,
            cpu_percent: 30.0,
            capture_storage_mb: 500,
            network_kb_per_minute: 100,
            chromium_instances: 1,
            concurrent_captures: 3,
        };
        let decision = evaluate(&sample, &budget);
        assert_eq!(decision.pressure, PressureLevel::Critical);
        assert!(
            decision
                .actions
                .contains(&DegradationAction::PreferSemanticOverVisual)
        );
        assert!(
            decision
                .actions
                .contains(&DegradationAction::BlockChromiumEscalation)
        );
    }

    #[test]
    fn scheduler_never_exceeds_declared_budget() {
        let work = vec![
            WorkCandidate {
                id: "a".into(),
                priority: 100,
                estimated_cpu_ms: 60,
                estimated_memory_mb: 20,
                requires_chromium: false,
            },
            WorkCandidate {
                id: "b".into(),
                priority: 90,
                estimated_cpu_ms: 60,
                estimated_memory_mb: 20,
                requires_chromium: false,
            },
        ];
        let selected = schedule(&work, 100, 100, false);
        assert_eq!(selected.len(), 1);
    }
}
