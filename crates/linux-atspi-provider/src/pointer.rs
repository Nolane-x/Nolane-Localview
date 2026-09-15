use crate::{AtspiAccessibilityBusIncarnationRef, AtspiElementBinding, AtspiEndpoint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtspiPointerHitTest {
    Target(AtspiEndpoint),
    Other(AtspiEndpoint),
    Unavailable,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AtspiPointerEligibilityPermit {
    binding_revision: u64,
    acquisition_cut_ref: String,
    accessibility_bus_incarnation_ref: AtspiAccessibilityBusIncarnationRef,
}

impl AtspiPointerEligibilityPermit {
    pub(crate) fn new(binding: &AtspiElementBinding) -> Self {
        Self {
            binding_revision: binding.binding_revision(),
            acquisition_cut_ref: binding.acquisition_cut_ref().to_owned(),
            accessibility_bus_incarnation_ref: *binding.accessibility_bus_incarnation_ref(),
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
}
