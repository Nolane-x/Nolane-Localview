use std::sync::atomic::{AtomicU64, Ordering};

use atspi::StateSet;
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

use crate::{AtspiAccessibilityBusIncarnationRef, AtspiElementBinding};

static NEXT_OBSERVATION_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtspiSemanticDimension {
    StateSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtspiEventAssurance {
    CompleteForRequestedDimensions,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtspiEventReliabilityProfile {
    assurance: AtspiEventAssurance,
    direct_reconciliation_dimensions: Vec<AtspiSemanticDimension>,
}

impl AtspiEventReliabilityProfile {
    /// AT-SPI event delivery is toolkit-sensitive. Any semantic dimension whose
    /// freshness matters to authority is therefore marked as requiring direct
    /// reconciliation rather than treating event silence as current-state proof.
    pub fn linux_toolkit_default(
        dimensions: impl IntoIterator<Item = AtspiSemanticDimension>,
    ) -> Self {
        let mut direct_reconciliation_dimensions = Vec::new();
        for dimension in dimensions {
            if !direct_reconciliation_dimensions.contains(&dimension) {
                direct_reconciliation_dimensions.push(dimension);
            }
        }

        let assurance = if direct_reconciliation_dimensions.is_empty() {
            AtspiEventAssurance::CompleteForRequestedDimensions
        } else {
            AtspiEventAssurance::Incomplete
        };

        Self {
            assurance,
            direct_reconciliation_dimensions,
        }
    }

    pub const fn assurance(&self) -> AtspiEventAssurance {
        self.assurance
    }

    pub fn requires_direct_reconciliation_for(
        &self,
        dimension: AtspiSemanticDimension,
    ) -> bool {
        self.direct_reconciliation_dimensions.contains(&dimension)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtspiObservationOrigin {
    EventCache,
    DirectReconciliation,
}

/// Immutable AT-SPI state observation bound to one provider/target/bus/binding
/// identity and one coherent observation cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtspiStateObservation {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    accessibility_bus_incarnation_ref: AtspiAccessibilityBusIncarnationRef,
    binding_revision: u64,
    observation_revision: u64,
    snapshot_cut_ref: String,
    origin: AtspiObservationOrigin,
    states: StateSet,
}

impl AtspiStateObservation {
    pub(crate) fn from_direct_reconciliation(
        binding: &AtspiElementBinding,
        states: StateSet,
    ) -> Self {
        let observation_revision = NEXT_OBSERVATION_REVISION.fetch_add(1, Ordering::Relaxed);
        Self {
            provider_incarnation_ref: binding.provider_incarnation_ref().clone(),
            target_incarnation_ref: binding.target_incarnation_ref().clone(),
            accessibility_bus_incarnation_ref: *binding.accessibility_bus_incarnation_ref(),
            binding_revision: binding.binding_revision(),
            observation_revision,
            snapshot_cut_ref: format!("cut:linux-atspi:reconcile:{observation_revision}"),
            origin: AtspiObservationOrigin::DirectReconciliation,
            states,
        }
    }

    #[cfg(feature = "validation-state-injection")]
    pub(crate) fn from_event_cache_for_validation(
        binding: &AtspiElementBinding,
        snapshot_cut_ref: impl Into<String>,
        states: StateSet,
    ) -> Self {
        Self {
            provider_incarnation_ref: binding.provider_incarnation_ref().clone(),
            target_incarnation_ref: binding.target_incarnation_ref().clone(),
            accessibility_bus_incarnation_ref: *binding.accessibility_bus_incarnation_ref(),
            binding_revision: binding.binding_revision(),
            observation_revision: NEXT_OBSERVATION_REVISION.fetch_add(1, Ordering::Relaxed),
            snapshot_cut_ref: snapshot_cut_ref.into(),
            origin: AtspiObservationOrigin::EventCache,
            states,
        }
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn accessibility_bus_incarnation_ref(&self) -> &AtspiAccessibilityBusIncarnationRef {
        &self.accessibility_bus_incarnation_ref
    }

    pub const fn binding_revision(&self) -> u64 {
        self.binding_revision
    }

    pub const fn observation_revision(&self) -> u64 {
        self.observation_revision
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub const fn origin(&self) -> AtspiObservationOrigin {
        self.origin
    }

    pub fn states(&self) -> StateSet {
        self.states
    }

    pub(crate) fn matches_binding(&self, binding: &AtspiElementBinding) -> bool {
        self.provider_incarnation_ref == *binding.provider_incarnation_ref()
            && self.target_incarnation_ref == *binding.target_incarnation_ref()
            && self.accessibility_bus_incarnation_ref == *binding.accessibility_bus_incarnation_ref()
            && self.binding_revision == binding.binding_revision()
    }
}
