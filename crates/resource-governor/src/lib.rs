#![forbid(unsafe_code)]

mod retained;
pub use retained::*;

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
    pub hidden_surfaces: usize,
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
            hidden_surfaces: 4,
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
    pub hidden_surfaces: usize,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessResourceMetrics {
    pub memory_mb: u64,
    pub cpu_percent: f32,
}

pub fn normalize_process_metrics(
    memory_bytes: u64,
    raw_cpu_percent: f32,
    logical_cpus: usize,
) -> Option<ProcessResourceMetrics> {
    if !raw_cpu_percent.is_finite() || raw_cpu_percent < 0.0 {
        return None;
    }
    let bytes_per_mb = 1024_u64 * 1024;
    let memory_mb = memory_bytes.div_ceil(bytes_per_mb);
    let cpu_percent = raw_cpu_percent / logical_cpus.max(1) as f32;
    if !cpu_percent.is_finite() {
        return None;
    }
    Some(ProcessResourceMetrics {
        memory_mb,
        cpu_percent,
    })
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
    SuspendInactiveRenderSurfaces,
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
        ratio(
            sample.hidden_surfaces as f32,
            budget.hidden_surfaces.max(1) as f32,
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
    if sample.hidden_surfaces >= budget.hidden_surfaces.max(1) {
        actions.push(DegradationAction::SuspendInactiveRenderSurfaces);
        reasons.push("hidden surface budget reached".into());
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
        if value > 0.0 { f32::INFINITY } else { 0.0 }
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
    NativeSemanticObservation,
    NativeSemanticReconciliation,
    NativeSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiveResourceKind {
    ChromiumProcess,
    NativeSurface,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct LiveSurfaceIdentity {
    pub surface_kind: String,
    pub label: String,
    pub incarnation: u64,
}

impl LiveSurfaceIdentity {
    pub fn new(
        surface_kind: impl Into<String>,
        label: impl Into<String>,
        incarnation: u64,
    ) -> Self {
        Self {
            surface_kind: surface_kind.into(),
            label: label.into(),
            incarnation,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceVisibility {
    Visible,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceActivationError {
    ReservationMissing,
    KindMismatch,
    SurfaceIdentityMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceAdmissionDenial {
    pub work_kind: ResourceWorkKind,
    pub decision: GovernorDecision,
}

type ReservationKey = (String, String);

#[derive(Debug, Clone, PartialEq, Eq)]
enum LiveResourceState {
    ChromiumProcess,
    NativeSurface {
        identity: LiveSurfaceIdentity,
        visibility: SurfaceVisibility,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReservationState {
    Pending(ResourceWorkKind),
    Live(LiveResourceState),
}

#[derive(Debug)]
struct RuntimeGovernorState {
    budget: ResourceBudget,
    sample: RuntimeResourceSample,
    process_memory_mb: u64,
    process_cpu_percent: f32,
    reservations: BTreeMap<ReservationKey, ReservationState>,
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
                process_memory_mb: 0,
                process_cpu_percent: 0.0,
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

    pub fn update_process_metrics(&self, memory_mb: u64, cpu_percent: f32) -> bool {
        if !cpu_percent.is_finite() || cpu_percent < 0.0 {
            return false;
        }
        let mut state = lock(&self.inner);
        state.process_memory_mb = memory_mb;
        state.process_cpu_percent = cpu_percent;
        true
    }

    pub fn decision(&self) -> GovernorDecision {
        let state = lock(&self.inner);
        decision_for_state(&state)
    }

    pub fn check(
        &self,
        work_kind: ResourceWorkKind,
    ) -> Result<GovernorDecision, ResourceAdmissionDenial> {
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
        state
            .reservations
            .insert(key.clone(), ReservationState::Pending(work_kind));
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
            .retain(|(reserved_session, _), reservation| {
                matches!(reservation, ReservationState::Live(_)) || reserved_session != session_id
            });
        before.saturating_sub(state.reservations.len())
    }

    fn activate_key(
        &self,
        key: &ReservationKey,
        kind: LiveResourceKind,
    ) -> Result<(), ResourceActivationError> {
        if kind != LiveResourceKind::ChromiumProcess {
            return Err(ResourceActivationError::KindMismatch);
        }
        let mut state = lock(&self.inner);
        let Some(reservation) = state.reservations.get_mut(key) else {
            return Err(ResourceActivationError::ReservationMissing);
        };
        match reservation {
            ReservationState::Pending(ResourceWorkKind::Chromium) => {
                *reservation = ReservationState::Live(LiveResourceState::ChromiumProcess);
                Ok(())
            }
            ReservationState::Pending(_) => Err(ResourceActivationError::KindMismatch),
            ReservationState::Live(_) => Err(ResourceActivationError::ReservationMissing),
        }
    }

    fn activate_surface_key(
        &self,
        key: &ReservationKey,
        identity: LiveSurfaceIdentity,
        visibility: SurfaceVisibility,
    ) -> Result<(), ResourceActivationError> {
        let mut state = lock(&self.inner);
        let Some(reservation) = state.reservations.get_mut(key) else {
            return Err(ResourceActivationError::ReservationMissing);
        };
        match reservation {
            ReservationState::Pending(ResourceWorkKind::NativeSurface) => {
                *reservation = ReservationState::Live(LiveResourceState::NativeSurface {
                    identity,
                    visibility,
                });
                Ok(())
            }
            ReservationState::Pending(_) => Err(ResourceActivationError::KindMismatch),
            ReservationState::Live(_) => Err(ResourceActivationError::ReservationMissing),
        }
    }

    fn set_surface_visibility_key(
        &self,
        key: &ReservationKey,
        identity: &LiveSurfaceIdentity,
        visibility: SurfaceVisibility,
    ) -> Result<(), ResourceActivationError> {
        let mut state = lock(&self.inner);
        let Some(reservation) = state.reservations.get_mut(key) else {
            return Err(ResourceActivationError::ReservationMissing);
        };
        match reservation {
            ReservationState::Live(LiveResourceState::NativeSurface {
                identity: current,
                visibility: current_visibility,
            }) if current == identity => {
                *current_visibility = visibility;
                Ok(())
            }
            ReservationState::Live(LiveResourceState::NativeSurface { .. }) => {
                Err(ResourceActivationError::SurfaceIdentityMismatch)
            }
            _ => Err(ResourceActivationError::KindMismatch),
        }
    }

    fn release_key(&self, key: &ReservationKey) {
        lock(&self.inner).reservations.remove(key);
    }

    fn release_live_key(
        &self,
        key: &ReservationKey,
        kind: LiveResourceKind,
        surface_identity: Option<&LiveSurfaceIdentity>,
    ) {
        let mut state = lock(&self.inner);
        let should_remove = match state.reservations.get(key) {
            Some(ReservationState::Live(LiveResourceState::ChromiumProcess)) => {
                kind == LiveResourceKind::ChromiumProcess && surface_identity.is_none()
            }
            Some(ReservationState::Live(LiveResourceState::NativeSurface {
                identity: current,
                ..
            })) => kind == LiveResourceKind::NativeSurface && surface_identity == Some(current),
            _ => false,
        };
        if should_remove {
            state.reservations.remove(key);
        }
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

    pub fn activate_live(
        mut self,
        kind: LiveResourceKind,
    ) -> Result<LiveResourceLease, ResourceActivationError> {
        let key = self
            .key
            .as_ref()
            .cloned()
            .ok_or(ResourceActivationError::ReservationMissing)?;
        self.governor.activate_key(&key, kind)?;
        self.key.take();
        Ok(LiveResourceLease {
            governor: self.governor.clone(),
            key: Some(key),
            kind,
            surface_identity: None,
        })
    }

    pub fn activate_surface(
        mut self,
        identity: LiveSurfaceIdentity,
        visibility: SurfaceVisibility,
    ) -> Result<LiveResourceLease, ResourceActivationError> {
        let key = self
            .key
            .as_ref()
            .cloned()
            .ok_or(ResourceActivationError::ReservationMissing)?;
        self.governor
            .activate_surface_key(&key, identity.clone(), visibility)?;
        self.key.take();
        Ok(LiveResourceLease {
            governor: self.governor.clone(),
            key: Some(key),
            kind: LiveResourceKind::NativeSurface,
            surface_identity: Some(identity),
        })
    }
}

impl Drop for ResourceReservation {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.governor.release_key(&key);
        }
    }
}

#[must_use = "live resource leases must remain alive for the full owner lifecycle"]
#[derive(Debug)]
pub struct LiveResourceLease {
    governor: RuntimeResourceGovernor,
    key: Option<ReservationKey>,
    kind: LiveResourceKind,
    surface_identity: Option<LiveSurfaceIdentity>,
}

impl LiveResourceLease {
    pub fn release(mut self) {
        if let Some(key) = self.key.take() {
            self.governor
                .release_live_key(&key, self.kind, self.surface_identity.as_ref());
        }
    }

    pub fn set_surface_visibility(
        &self,
        identity: LiveSurfaceIdentity,
        visibility: SurfaceVisibility,
    ) -> Result<(), ResourceActivationError> {
        if self.kind != LiveResourceKind::NativeSurface {
            return Err(ResourceActivationError::KindMismatch);
        }
        let key = self
            .key
            .as_ref()
            .ok_or(ResourceActivationError::ReservationMissing)?;
        self.governor
            .set_surface_visibility_key(key, &identity, visibility)
    }
}

impl Drop for LiveResourceLease {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.governor
                .release_live_key(&key, self.kind, self.surface_identity.as_ref());
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
    let mut hidden_surfaces = 0usize;
    for reservation in state.reservations.values() {
        match reservation {
            ReservationState::Pending(ResourceWorkKind::NativeVisualCapture) => {
                concurrent_captures = concurrent_captures.saturating_add(1)
            }
            ReservationState::Pending(ResourceWorkKind::Chromium)
            | ReservationState::Live(LiveResourceState::ChromiumProcess) => {
                chromium_instances = chromium_instances.saturating_add(1)
            }
            ReservationState::Pending(ResourceWorkKind::NativeSurface) => {
                hidden_surfaces = hidden_surfaces.saturating_add(1)
            }
            ReservationState::Live(LiveResourceState::NativeSurface {
                visibility: SurfaceVisibility::Hidden,
                ..
            }) => hidden_surfaces = hidden_surfaces.saturating_add(1),
            ReservationState::Live(LiveResourceState::NativeSurface {
                visibility: SurfaceVisibility::Visible,
                ..
            }) => {}
            ReservationState::Pending(ResourceWorkKind::NativeSemanticObservation)
            | ReservationState::Pending(ResourceWorkKind::NativeSemanticReconciliation) => {}
        }
    }
    let cpu_percent = ((state.sample.cpu_percent as f64) + (state.process_cpu_percent as f64))
        .min(f32::MAX as f64) as f32;
    evaluate(
        &ResourceSample {
            memory_mb: state
                .sample
                .memory_mb
                .saturating_add(state.process_memory_mb),
            cpu_percent,
            capture_storage_mb: state.sample.capture_storage_mb,
            network_kb_per_minute: state.sample.network_kb_per_minute,
            chromium_instances,
            concurrent_captures,
            hidden_surfaces,
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
        ResourceWorkKind::NativeSurface => {
            decision.pressure >= PressureLevel::High
                || decision
                    .actions
                    .contains(&DegradationAction::SuspendInactiveRenderSurfaces)
        }
        ResourceWorkKind::NativeSemanticObservation
        | ResourceWorkKind::NativeSemanticReconciliation => {
            decision.pressure == PressureLevel::Critical
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
            hidden_surfaces: 0,
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
