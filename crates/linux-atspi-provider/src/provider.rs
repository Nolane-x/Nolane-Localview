use atspi::{State, StateSet};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

use crate::{
    AtspiActionEligibilityError, AtspiActionEligibilityPermit, AtspiBindingLifecycle,
    AtspiElementBinding, AtspiEndpoint,
};

#[derive(Debug, Clone)]
pub struct LinuxAtspiProvider {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
}

impl LinuxAtspiProvider {
    #[cfg(feature = "validation-state-injection")]
    pub fn new_for_validation(
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
    ) -> Self {
        Self {
            provider_incarnation_ref,
            target_incarnation_ref,
        }
    }

    pub fn bind(
        &self,
        endpoint: AtspiEndpoint,
        acquisition_cut_ref: impl Into<String>,
    ) -> AtspiElementBinding {
        AtspiElementBinding::new(
            self.provider_incarnation_ref.clone(),
            self.target_incarnation_ref.clone(),
            endpoint,
            acquisition_cut_ref,
        )
    }

    pub fn reacquire(
        &self,
        endpoint: AtspiEndpoint,
        acquisition_cut_ref: impl Into<String>,
    ) -> AtspiElementBinding {
        self.bind(endpoint, acquisition_cut_ref)
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn authorize_from_state_set_for_validation(
        &self,
        binding: &AtspiElementBinding,
        states: StateSet,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        self.authorize_from_state_set(binding, states)
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn authorize_unavailable_for_validation(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        if binding.lifecycle() == AtspiBindingLifecycle::InvalidDefunct {
            return Err(AtspiActionEligibilityError::AlreadyInvalidDefunct);
        }
        if binding.provider_incarnation_ref() != &self.provider_incarnation_ref {
            return Err(AtspiActionEligibilityError::ProviderIncarnationMismatch);
        }
        if binding.target_incarnation_ref() != &self.target_incarnation_ref {
            return Err(AtspiActionEligibilityError::TargetIncarnationMismatch);
        }

        Err(AtspiActionEligibilityError::ObservationUnavailable)
    }

    fn authorize_from_state_set(
        &self,
        binding: &AtspiElementBinding,
        states: StateSet,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        if binding.lifecycle() == AtspiBindingLifecycle::InvalidDefunct {
            return Err(AtspiActionEligibilityError::AlreadyInvalidDefunct);
        }
        if binding.provider_incarnation_ref() != &self.provider_incarnation_ref {
            return Err(AtspiActionEligibilityError::ProviderIncarnationMismatch);
        }
        if binding.target_incarnation_ref() != &self.target_incarnation_ref {
            return Err(AtspiActionEligibilityError::TargetIncarnationMismatch);
        }
        if states.contains(State::Defunct) {
            binding.invalidate_defunct();
            return Err(AtspiActionEligibilityError::Defunct);
        }

        Ok(AtspiActionEligibilityPermit::new(binding))
    }
}
