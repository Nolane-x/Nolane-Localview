use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

use crate::{AtspiAccessibilityBusIncarnationRef, AtspiStateObservation};

static NEXT_BINDING_REVISION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AtspiEndpoint {
    bus_name: String,
    object_path: String,
}

impl AtspiEndpoint {
    pub fn new(bus_name: impl Into<String>, object_path: impl Into<String>) -> Self {
        Self {
            bus_name: bus_name.into(),
            object_path: object_path.into(),
        }
    }

    pub fn bus_name(&self) -> &str {
        &self.bus_name
    }

    pub fn object_path(&self) -> &str {
        &self.object_path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtspiBindingLifecycle {
    Live,
    InvalidDefunct,
}

#[derive(Debug)]
struct AtspiBindingState {
    lifecycle: AtspiBindingLifecycle,
}

#[derive(Debug, Clone)]
pub struct AtspiElementBinding {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    accessibility_bus_incarnation_ref: AtspiAccessibilityBusIncarnationRef,
    endpoint: AtspiEndpoint,
    acquisition_cut_ref: String,
    binding_revision: u64,
    state: Arc<Mutex<AtspiBindingState>>,
}

impl AtspiElementBinding {
    pub(crate) fn new(
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
        accessibility_bus_incarnation_ref: AtspiAccessibilityBusIncarnationRef,
        endpoint: AtspiEndpoint,
        acquisition_cut_ref: impl Into<String>,
    ) -> Self {
        Self {
            provider_incarnation_ref,
            target_incarnation_ref,
            accessibility_bus_incarnation_ref,
            endpoint,
            acquisition_cut_ref: acquisition_cut_ref.into(),
            binding_revision: NEXT_BINDING_REVISION.fetch_add(1, Ordering::Relaxed),
            state: Arc::new(Mutex::new(AtspiBindingState {
                lifecycle: AtspiBindingLifecycle::Live,
            })),
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

    pub fn endpoint(&self) -> &AtspiEndpoint {
        &self.endpoint
    }

    pub fn acquisition_cut_ref(&self) -> &str {
        &self.acquisition_cut_ref
    }

    pub fn binding_revision(&self) -> u64 {
        self.binding_revision
    }

    pub fn lifecycle(&self) -> AtspiBindingLifecycle {
        self.state
            .lock()
            .expect("AT-SPI binding lifecycle mutex poisoned")
            .lifecycle
    }

    pub(crate) fn invalidate_defunct(&self) {
        self.state
            .lock()
            .expect("AT-SPI binding lifecycle mutex poisoned")
            .lifecycle = AtspiBindingLifecycle::InvalidDefunct;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AtspiActionEligibilityPermit {
    binding_revision: u64,
    acquisition_cut_ref: String,
    accessibility_bus_incarnation_ref: AtspiAccessibilityBusIncarnationRef,
    observation_revision: Option<u64>,
    observation_cut_ref: Option<String>,
}

impl AtspiActionEligibilityPermit {
    pub(crate) fn new(binding: &AtspiElementBinding) -> Self {
        Self {
            binding_revision: binding.binding_revision(),
            acquisition_cut_ref: binding.acquisition_cut_ref().to_owned(),
            accessibility_bus_incarnation_ref: *binding.accessibility_bus_incarnation_ref(),
            observation_revision: None,
            observation_cut_ref: None,
        }
    }

    pub(crate) fn new_reconciled(
        binding: &AtspiElementBinding,
        observation: &AtspiStateObservation,
    ) -> Self {
        Self {
            binding_revision: binding.binding_revision(),
            acquisition_cut_ref: binding.acquisition_cut_ref().to_owned(),
            accessibility_bus_incarnation_ref: *binding.accessibility_bus_incarnation_ref(),
            observation_revision: Some(observation.observation_revision()),
            observation_cut_ref: Some(observation.snapshot_cut_ref().to_owned()),
        }
    }

    pub fn binding_revision(&self) -> u64 {
        self.binding_revision
    }

    pub fn acquisition_cut_ref(&self) -> &str {
        &self.acquisition_cut_ref
    }

    pub fn accessibility_bus_incarnation_ref(&self) -> &AtspiAccessibilityBusIncarnationRef {
        &self.accessibility_bus_incarnation_ref
    }

    pub const fn observation_revision(&self) -> Option<u64> {
        self.observation_revision
    }

    pub fn observation_cut_ref(&self) -> Option<&str> {
        self.observation_cut_ref.as_deref()
    }
}
