use atspi::{AccessibilityConnection, State, StateSet, proxy::accessible::AccessibleProxy};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

use crate::{
    AtspiActionEligibilityError, AtspiActionEligibilityPermit, AtspiBindingLifecycle,
    AtspiElementBinding, AtspiEndpoint, AtspiProviderConnectionError,
};

#[derive(Debug, Clone)]
pub struct LinuxAtspiProvider {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    connection: Option<AccessibilityConnection>,
}

impl LinuxAtspiProvider {
    pub async fn connect(
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
    ) -> Result<Self, AtspiProviderConnectionError> {
        let connection = AccessibilityConnection::new()
            .await
            .map_err(|_| AtspiProviderConnectionError::AccessibilityBusUnavailable)?;

        Ok(Self {
            provider_incarnation_ref,
            target_incarnation_ref,
            connection: Some(connection),
        })
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn new_for_validation(
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
    ) -> Self {
        Self {
            provider_incarnation_ref,
            target_incarnation_ref,
            connection: None,
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

    pub async fn authorize_action(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;

        let connection = self
            .connection
            .as_ref()
            .ok_or(AtspiActionEligibilityError::ObservationUnavailable)?;
        let proxy = AccessibleProxy::builder(connection.connection())
            .destination(binding.endpoint().bus_name())
            .map_err(|_| AtspiActionEligibilityError::ObservationUnavailable)?
            .path(binding.endpoint().object_path())
            .map_err(|_| AtspiActionEligibilityError::ObservationUnavailable)?
            .build()
            .await
            .map_err(|_| AtspiActionEligibilityError::ObservationUnavailable)?;
        let states = proxy
            .get_state()
            .await
            .map_err(|_| AtspiActionEligibilityError::ObservationUnavailable)?;

        self.authorize_from_state_set(binding, states)
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
        self.ensure_binding_authority(binding)?;
        Err(AtspiActionEligibilityError::ObservationUnavailable)
    }

    fn ensure_binding_authority(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<(), AtspiActionEligibilityError> {
        if binding.lifecycle() == AtspiBindingLifecycle::InvalidDefunct {
            return Err(AtspiActionEligibilityError::AlreadyInvalidDefunct);
        }
        if binding.provider_incarnation_ref() != &self.provider_incarnation_ref {
            return Err(AtspiActionEligibilityError::ProviderIncarnationMismatch);
        }
        if binding.target_incarnation_ref() != &self.target_incarnation_ref {
            return Err(AtspiActionEligibilityError::TargetIncarnationMismatch);
        }
        Ok(())
    }

    fn authorize_from_state_set(
        &self,
        binding: &AtspiElementBinding,
        states: StateSet,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;
        if states.contains(State::Defunct) {
            binding.invalidate_defunct();
            return Err(AtspiActionEligibilityError::Defunct);
        }

        Ok(AtspiActionEligibilityPermit::new(binding))
    }
}
