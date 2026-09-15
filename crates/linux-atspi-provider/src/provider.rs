use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use atspi::{
    AccessibilityConnection, CoordType, Layer, State, StateSet,
    proxy::{accessible::AccessibleProxy, component::ComponentProxy},
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};

use crate::{
    AtspiAccessibilityBusIncarnationRef, AtspiAccessibilityBusLifecycle,
    AtspiActionEligibilityError, AtspiActionEligibilityPermit, AtspiBindError,
    AtspiBindingLifecycle, AtspiElementBinding, AtspiEndpoint, AtspiEventReliabilityProfile,
    AtspiObservationOrigin, AtspiPointerEligibilityError, AtspiPointerEligibilityPermit,
    AtspiPointerHitTest, AtspiProviderConnectionError, AtspiReacquireError,
    AtspiSemanticDimension, AtspiStateObservation,
};

#[derive(Debug, Clone, Copy)]
struct EndpointAuthorityRecord {
    binding_revision: u64,
    retired: bool,
}

#[derive(Debug)]
struct AccessibilityBusRuntime {
    lifecycle: AtspiAccessibilityBusLifecycle,
    incarnation_ref: AtspiAccessibilityBusIncarnationRef,
    connection: Option<AccessibilityConnection>,
}

fn layer_stack_rank(layer: Layer) -> Option<u8> {
    match layer {
        Layer::Invalid => None,
        Layer::Background => Some(1),
        Layer::Window => Some(2),
        Layer::Mdi => Some(3),
        Layer::Canvas => Some(4),
        Layer::Widget => Some(5),
        Layer::Popup => Some(6),
        Layer::Overlay => Some(7),
    }
}

fn point_in_extents(extents: (i32, i32, i32, i32), point: (i32, i32)) -> bool {
    let (x, y, width, height) = extents;
    if width <= 0 || height <= 0 {
        return false;
    }

    let left = i64::from(x);
    let top = i64::from(y);
    let right = left + i64::from(width);
    let bottom = top + i64::from(height);
    let px = i64::from(point.0);
    let py = i64::from(point.1);
    px >= left && px < right && py >= top && py < bottom
}

#[derive(Debug, Clone)]
pub struct LinuxAtspiProvider {
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    accessibility_bus: Arc<Mutex<AccessibilityBusRuntime>>,
    endpoint_authority: Arc<Mutex<HashMap<AtspiEndpoint, EndpointAuthorityRecord>>>,
    validation_only: bool,
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
            accessibility_bus: Arc::new(Mutex::new(AccessibilityBusRuntime {
                lifecycle: AtspiAccessibilityBusLifecycle::Connected,
                incarnation_ref: AtspiAccessibilityBusIncarnationRef::fresh(),
                connection: Some(connection),
            })),
            endpoint_authority: Arc::new(Mutex::new(HashMap::new())),
            validation_only: false,
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
            accessibility_bus: Arc::new(Mutex::new(AccessibilityBusRuntime {
                lifecycle: AtspiAccessibilityBusLifecycle::Connected,
                incarnation_ref: AtspiAccessibilityBusIncarnationRef::fresh(),
                connection: None,
            })),
            endpoint_authority: Arc::new(Mutex::new(HashMap::new())),
            validation_only: true,
        }
    }

    pub fn accessibility_bus_lifecycle(&self) -> AtspiAccessibilityBusLifecycle {
        self.refresh_closed_transport_state();
        self.accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned")
            .lifecycle
    }

    pub fn accessibility_bus_incarnation_ref(&self) -> AtspiAccessibilityBusIncarnationRef {
        self.refresh_closed_transport_state();
        self.accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned")
            .incarnation_ref
    }

    pub fn mark_accessibility_bus_disconnected(&self) {
        let mut bus = self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned");
        bus.lifecycle = AtspiAccessibilityBusLifecycle::Disconnected;
        bus.connection = None;
    }

    pub async fn reconnect_accessibility_bus(
        &self,
    ) -> Result<AtspiAccessibilityBusIncarnationRef, AtspiProviderConnectionError> {
        {
            let bus = self
                .accessibility_bus
                .lock()
                .expect("AT-SPI accessibility bus mutex poisoned");
            if bus.lifecycle == AtspiAccessibilityBusLifecycle::Connected {
                return Err(AtspiProviderConnectionError::AccessibilityBusStillConnected);
            }
        }

        if self.validation_only {
            return Err(AtspiProviderConnectionError::AccessibilityBusUnavailable);
        }

        let connection = AccessibilityConnection::new()
            .await
            .map_err(|_| AtspiProviderConnectionError::AccessibilityBusUnavailable)?;

        let mut bus = self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned");
        if bus.lifecycle == AtspiAccessibilityBusLifecycle::Connected {
            return Err(AtspiProviderConnectionError::AccessibilityBusStillConnected);
        }
        let fresh = AtspiAccessibilityBusIncarnationRef::fresh();
        bus.incarnation_ref = fresh;
        bus.connection = Some(connection);
        bus.lifecycle = AtspiAccessibilityBusLifecycle::Connected;
        Ok(fresh)
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn reconnect_accessibility_bus_for_validation(
        &self,
    ) -> Result<AtspiAccessibilityBusIncarnationRef, AtspiProviderConnectionError> {
        let mut bus = self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned");
        if bus.lifecycle == AtspiAccessibilityBusLifecycle::Connected {
            return Err(AtspiProviderConnectionError::AccessibilityBusStillConnected);
        }
        let fresh = AtspiAccessibilityBusIncarnationRef::fresh();
        bus.incarnation_ref = fresh;
        bus.connection = None;
        bus.lifecycle = AtspiAccessibilityBusLifecycle::Connected;
        Ok(fresh)
    }

    pub fn bind_initial(
        &self,
        endpoint: AtspiEndpoint,
        acquisition_cut_ref: impl Into<String>,
    ) -> Result<AtspiElementBinding, AtspiBindError> {
        self.refresh_closed_transport_state();
        let bus_incarnation = {
            let bus = self
                .accessibility_bus
                .lock()
                .expect("AT-SPI accessibility bus mutex poisoned");
            if bus.lifecycle == AtspiAccessibilityBusLifecycle::Disconnected {
                return Err(AtspiBindError::AccessibilityBusDisconnected);
            }
            bus.incarnation_ref
        };

        let mut authority = self
            .endpoint_authority
            .lock()
            .expect("AT-SPI endpoint authority mutex poisoned");
        if authority.contains_key(&endpoint) {
            return Err(AtspiBindError::EndpointAlreadyBound);
        }

        let binding = AtspiElementBinding::new(
            self.provider_incarnation_ref.clone(),
            self.target_incarnation_ref.clone(),
            bus_incarnation,
            endpoint.clone(),
            acquisition_cut_ref,
        );
        authority.insert(
            endpoint,
            EndpointAuthorityRecord {
                binding_revision: binding.binding_revision(),
                retired: false,
            },
        );
        Ok(binding)
    }

    pub fn reacquire_after_defunct(
        &self,
        previous_binding: &AtspiElementBinding,
        replacement_endpoint: AtspiEndpoint,
        acquisition_cut_ref: impl Into<String>,
    ) -> Result<AtspiElementBinding, AtspiReacquireError> {
        if previous_binding.provider_incarnation_ref() != &self.provider_incarnation_ref {
            return Err(AtspiReacquireError::ProviderIncarnationMismatch);
        }
        if previous_binding.target_incarnation_ref() != &self.target_incarnation_ref {
            return Err(AtspiReacquireError::TargetIncarnationMismatch);
        }
        if previous_binding.lifecycle() != AtspiBindingLifecycle::InvalidDefunct {
            return Err(AtspiReacquireError::PreviousBindingNotDefunct);
        }

        self.refresh_closed_transport_state();
        let bus_incarnation = {
            let bus = self
                .accessibility_bus
                .lock()
                .expect("AT-SPI accessibility bus mutex poisoned");
            if bus.lifecycle == AtspiAccessibilityBusLifecycle::Disconnected {
                return Err(AtspiReacquireError::AccessibilityBusDisconnected);
            }
            if previous_binding.accessibility_bus_incarnation_ref() != &bus.incarnation_ref {
                return Err(AtspiReacquireError::AccessibilityBusIncarnationMismatch);
            }
            bus.incarnation_ref
        };

        let previous_endpoint = previous_binding.endpoint().clone();
        let mut authority = self
            .endpoint_authority
            .lock()
            .expect("AT-SPI endpoint authority mutex poisoned");
        let previous_record = authority
            .get(&previous_endpoint)
            .copied()
            .ok_or(AtspiReacquireError::PreviousBindingSuperseded)?;
        if previous_record.retired
            || previous_record.binding_revision != previous_binding.binding_revision()
        {
            return Err(AtspiReacquireError::PreviousBindingSuperseded);
        }
        if replacement_endpoint != previous_endpoint && authority.contains_key(&replacement_endpoint)
        {
            return Err(AtspiReacquireError::ReplacementEndpointAlreadyBound);
        }

        let fresh = AtspiElementBinding::new(
            self.provider_incarnation_ref.clone(),
            self.target_incarnation_ref.clone(),
            bus_incarnation,
            replacement_endpoint.clone(),
            acquisition_cut_ref,
        );

        if replacement_endpoint == previous_endpoint {
            authority.insert(
                replacement_endpoint,
                EndpointAuthorityRecord {
                    binding_revision: fresh.binding_revision(),
                    retired: false,
                },
            );
        } else {
            authority.insert(
                previous_endpoint,
                EndpointAuthorityRecord {
                    binding_revision: previous_record.binding_revision,
                    retired: true,
                },
            );
            authority.insert(
                replacement_endpoint,
                EndpointAuthorityRecord {
                    binding_revision: fresh.binding_revision(),
                    retired: false,
                },
            );
        }

        Ok(fresh)
    }

    pub fn reacquire_after_bus_reconnect(
        &self,
        previous_binding: &AtspiElementBinding,
        replacement_endpoint: AtspiEndpoint,
        acquisition_cut_ref: impl Into<String>,
    ) -> Result<AtspiElementBinding, AtspiReacquireError> {
        if previous_binding.provider_incarnation_ref() != &self.provider_incarnation_ref {
            return Err(AtspiReacquireError::ProviderIncarnationMismatch);
        }
        if previous_binding.target_incarnation_ref() != &self.target_incarnation_ref {
            return Err(AtspiReacquireError::TargetIncarnationMismatch);
        }

        self.refresh_closed_transport_state();
        let current_bus = {
            let bus = self
                .accessibility_bus
                .lock()
                .expect("AT-SPI accessibility bus mutex poisoned");
            if bus.lifecycle == AtspiAccessibilityBusLifecycle::Disconnected {
                return Err(AtspiReacquireError::AccessibilityBusDisconnected);
            }
            if previous_binding.accessibility_bus_incarnation_ref() == &bus.incarnation_ref {
                return Err(AtspiReacquireError::AccessibilityBusIncarnationNotAdvanced);
            }
            bus.incarnation_ref
        };

        let previous_endpoint = previous_binding.endpoint().clone();
        let mut authority = self
            .endpoint_authority
            .lock()
            .expect("AT-SPI endpoint authority mutex poisoned");
        let previous_record = authority
            .get(&previous_endpoint)
            .copied()
            .ok_or(AtspiReacquireError::PreviousBindingSuperseded)?;
        if previous_record.retired
            || previous_record.binding_revision != previous_binding.binding_revision()
        {
            return Err(AtspiReacquireError::PreviousBindingSuperseded);
        }
        if replacement_endpoint != previous_endpoint && authority.contains_key(&replacement_endpoint)
        {
            return Err(AtspiReacquireError::ReplacementEndpointAlreadyBound);
        }

        let fresh = AtspiElementBinding::new(
            self.provider_incarnation_ref.clone(),
            self.target_incarnation_ref.clone(),
            current_bus,
            replacement_endpoint.clone(),
            acquisition_cut_ref,
        );

        if replacement_endpoint == previous_endpoint {
            authority.insert(
                replacement_endpoint,
                EndpointAuthorityRecord {
                    binding_revision: fresh.binding_revision(),
                    retired: false,
                },
            );
        } else {
            authority.insert(
                previous_endpoint,
                EndpointAuthorityRecord {
                    binding_revision: previous_record.binding_revision,
                    retired: true,
                },
            );
            authority.insert(
                replacement_endpoint,
                EndpointAuthorityRecord {
                    binding_revision: fresh.binding_revision(),
                    retired: false,
                },
            );
        }

        Ok(fresh)
    }

    /// Perform a fresh direct AT-SPI state read and mint an immutable
    /// observation revision/cut. This is the reconciliation path used when
    /// toolkit event delivery cannot prove current state completeness.
    pub async fn reconcile_action_state(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<AtspiStateObservation, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;
        let connection = self.connection_for_live_authority()?;
        let proxy = AccessibleProxy::builder(connection.connection())
            .destination(binding.endpoint().bus_name())
            .map_err(|_| self.observation_transport_error())?
            .path(binding.endpoint().object_path())
            .map_err(|_| self.observation_transport_error())?
            .build()
            .await
            .map_err(|_| self.observation_transport_error())?;
        let states = proxy
            .get_state()
            .await
            .map_err(|_| self.observation_transport_error())?;

        Ok(AtspiStateObservation::from_direct_reconciliation(
            binding, states,
        ))
    }

    pub async fn authorize_action(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        let observation = self.reconcile_action_state(binding).await?;
        let reliability = AtspiEventReliabilityProfile::linux_toolkit_default([
            AtspiSemanticDimension::StateSet,
        ]);
        self.authorize_action_from_observation(binding, &reliability, &observation)
    }

    pub async fn authorize_pointer_action(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<AtspiPointerEligibilityPermit, AtspiPointerEligibilityError> {
        self.ensure_binding_authority(binding)
            .map_err(AtspiPointerEligibilityError::Semantic)?;

        let connection = self
            .connection_for_live_authority()
            .map_err(AtspiPointerEligibilityError::Semantic)?;
        let bus = connection.connection();
        let accessible = AccessibleProxy::builder(bus)
            .destination(binding.endpoint().bus_name())
            .map_err(|_| {
                AtspiPointerEligibilityError::Semantic(self.observation_transport_error())
            })?
            .path(binding.endpoint().object_path())
            .map_err(|_| {
                AtspiPointerEligibilityError::Semantic(self.observation_transport_error())
            })?
            .build()
            .await
            .map_err(|_| {
                AtspiPointerEligibilityError::Semantic(self.observation_transport_error())
            })?;
        let states = accessible.get_state().await.map_err(|_| {
            AtspiPointerEligibilityError::Semantic(self.observation_transport_error())
        })?;

        self.authorize_from_state_set(binding, states)
            .map_err(AtspiPointerEligibilityError::Semantic)?;
        if !states.contains(State::Visible) {
            return Err(AtspiPointerEligibilityError::NotVisible);
        }
        if !states.contains(State::Showing) {
            return Err(AtspiPointerEligibilityError::NotShowing);
        }

        let component = ComponentProxy::builder(bus)
            .destination(binding.endpoint().bus_name())
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
            .path(binding.endpoint().object_path())
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
            .build()
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        let (x, y, width, height) = component
            .get_extents(CoordType::Screen)
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        if width <= 0 || height <= 0 {
            return Err(AtspiPointerEligibilityError::HitTestUnavailable);
        }
        let center_x = x
            .checked_add(width / 2)
            .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
        let center_y = y
            .checked_add(height / 2)
            .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
        let center = (center_x, center_y);

        let target_layer = component
            .get_layer()
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        let target_layer_rank =
            layer_stack_rank(target_layer).ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
        let target_mdi_z = if matches!(target_layer, Layer::Window | Layer::Mdi) {
            Some(
                component
                    .get_mdiz_order()
                    .await
                    .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?,
            )
        } else {
            None
        };

        let parent = accessible
            .parent()
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        if parent.is_null() {
            return Err(AtspiPointerEligibilityError::HitTestUnavailable);
        }
        let parent_bus_name = parent
            .name_as_str()
            .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
        let parent_path = parent.path_as_str();
        let parent_component = ComponentProxy::builder(bus)
            .destination(parent_bus_name)
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
            .path(parent_path)
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
            .build()
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        let hit = parent_component
            .get_accessible_at_point(center_x, center_y, CoordType::Screen)
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        if hit.is_null() {
            return Err(AtspiPointerEligibilityError::HitTestUnavailable);
        }
        let hit_bus_name = hit
            .name_as_str()
            .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
        let hit_endpoint = AtspiEndpoint::new(hit_bus_name, hit.path_as_str());
        if &hit_endpoint != binding.endpoint() {
            return self.authorize_pointer_from_observation(
                binding,
                states,
                AtspiPointerHitTest::Other(hit_endpoint),
            );
        }

        // A target self-hit is necessary but not sufficient. AT-SPI documents
        // explicit layer/z-order authority and, for ordinary same-layer siblings,
        // recommends "first child paints first". Inspect every overlapping
        // sibling for higher layer/z-order, and use child order only as the
        // same-layer tie-breaker where no explicit z-order exists.
        let parent_accessible = AccessibleProxy::builder(bus)
            .destination(parent_bus_name)
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
            .path(parent_path)
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
            .build()
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        let children = parent_accessible
            .get_children()
            .await
            .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
        let target_index = children
            .iter()
            .position(|child| {
                child.name_as_str() == Some(binding.endpoint().bus_name())
                    && child.path_as_str() == binding.endpoint().object_path()
            })
            .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;

        for (sibling_index, sibling) in children.iter().enumerate() {
            if sibling_index == target_index || sibling.is_null() {
                continue;
            }
            let sibling_bus_name = sibling
                .name_as_str()
                .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
            let sibling_path = sibling.path_as_str();
            let sibling_accessible = AccessibleProxy::builder(bus)
                .destination(sibling_bus_name)
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
                .path(sibling_path)
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
                .build()
                .await
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
            let sibling_states = sibling_accessible
                .get_state()
                .await
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
            if sibling_states.contains(State::Defunct)
                || !sibling_states.contains(State::Visible)
                || !sibling_states.contains(State::Showing)
            {
                continue;
            }

            let sibling_component = ComponentProxy::builder(bus)
                .destination(sibling_bus_name)
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
                .path(sibling_path)
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?
                .build()
                .await
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
            let sibling_extents = sibling_component
                .get_extents(CoordType::Screen)
                .await
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
            if !point_in_extents(sibling_extents, center) {
                continue;
            }

            let sibling_layer = sibling_component
                .get_layer()
                .await
                .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
            let sibling_layer_rank = layer_stack_rank(sibling_layer)
                .ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
            if sibling_layer_rank > target_layer_rank {
                return Err(AtspiPointerEligibilityError::Occluded);
            }
            if sibling_layer_rank < target_layer_rank {
                continue;
            }

            if matches!(target_layer, Layer::Window | Layer::Mdi) {
                let sibling_z = sibling_component
                    .get_mdiz_order()
                    .await
                    .map_err(|_| AtspiPointerEligibilityError::HitTestUnavailable)?;
                let target_z =
                    target_mdi_z.ok_or(AtspiPointerEligibilityError::HitTestUnavailable)?;
                if sibling_z > target_z || (sibling_z == target_z && sibling_index > target_index) {
                    return Err(AtspiPointerEligibilityError::Occluded);
                }
            } else if sibling_index > target_index {
                return Err(AtspiPointerEligibilityError::Occluded);
            }
        }

        self.authorize_pointer_from_observation(
            binding,
            states,
            AtspiPointerHitTest::Target(hit_endpoint),
        )
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn state_observation_from_event_for_validation(
        &self,
        binding: &AtspiElementBinding,
        snapshot_cut_ref: impl Into<String>,
        states: StateSet,
    ) -> Result<AtspiStateObservation, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;
        Ok(AtspiStateObservation::from_event_cache_for_validation(
            binding,
            snapshot_cut_ref,
            states,
        ))
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn reconcile_state_from_state_set_for_validation(
        &self,
        binding: &AtspiElementBinding,
        states: StateSet,
    ) -> Result<AtspiStateObservation, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;
        Ok(AtspiStateObservation::from_direct_reconciliation(
            binding, states,
        ))
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn authorize_action_from_observation_for_validation(
        &self,
        binding: &AtspiElementBinding,
        reliability: &AtspiEventReliabilityProfile,
        observation: &AtspiStateObservation,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        self.authorize_action_from_observation(binding, reliability, observation)
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
    pub fn authorize_pointer_from_observation_for_validation(
        &self,
        binding: &AtspiElementBinding,
        states: StateSet,
        hit_test: AtspiPointerHitTest,
    ) -> Result<AtspiPointerEligibilityPermit, AtspiPointerEligibilityError> {
        self.authorize_pointer_from_observation(binding, states, hit_test)
    }

    #[cfg(feature = "validation-state-injection")]
    pub fn authorize_unavailable_for_validation(
        &self,
        binding: &AtspiElementBinding,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;
        Err(AtspiActionEligibilityError::ObservationUnavailable)
    }

    fn refresh_closed_transport_state(&self) {
        let connection = {
            let bus = self
                .accessibility_bus
                .lock()
                .expect("AT-SPI accessibility bus mutex poisoned");
            if bus.lifecycle == AtspiAccessibilityBusLifecycle::Disconnected {
                return;
            }
            bus.connection.clone()
        };

        let Some(connection) = connection else {
            return;
        };
        if !connection.connection().is_closed() {
            return;
        }

        let mut bus = self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned");
        if bus
            .connection
            .as_ref()
            .is_some_and(|current| current.connection().is_closed())
        {
            bus.lifecycle = AtspiAccessibilityBusLifecycle::Disconnected;
            bus.connection = None;
        }
    }

    fn connection_for_live_authority(
        &self,
    ) -> Result<AccessibilityConnection, AtspiActionEligibilityError> {
        self.refresh_closed_transport_state();
        let bus = self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned");
        if bus.lifecycle == AtspiAccessibilityBusLifecycle::Disconnected {
            return Err(AtspiActionEligibilityError::AccessibilityBusDisconnected);
        }
        bus.connection
            .clone()
            .ok_or(AtspiActionEligibilityError::ObservationUnavailable)
    }

    fn observation_transport_error(&self) -> AtspiActionEligibilityError {
        self.refresh_closed_transport_state();
        if self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned")
            .lifecycle
            == AtspiAccessibilityBusLifecycle::Disconnected
        {
            AtspiActionEligibilityError::AccessibilityBusDisconnected
        } else {
            AtspiActionEligibilityError::ObservationUnavailable
        }
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

        self.refresh_closed_transport_state();
        let bus = self
            .accessibility_bus
            .lock()
            .expect("AT-SPI accessibility bus mutex poisoned");
        if bus.lifecycle == AtspiAccessibilityBusLifecycle::Disconnected {
            return Err(AtspiActionEligibilityError::AccessibilityBusDisconnected);
        }
        if binding.accessibility_bus_incarnation_ref() != &bus.incarnation_ref {
            return Err(AtspiActionEligibilityError::AccessibilityBusIncarnationMismatch);
        }
        Ok(())
    }

    fn authorize_action_from_observation(
        &self,
        binding: &AtspiElementBinding,
        reliability: &AtspiEventReliabilityProfile,
        observation: &AtspiStateObservation,
    ) -> Result<AtspiActionEligibilityPermit, AtspiActionEligibilityError> {
        self.ensure_binding_authority(binding)?;
        if !observation.matches_binding(binding) {
            return Err(AtspiActionEligibilityError::ObservationBindingMismatch);
        }
        if reliability.requires_direct_reconciliation_for(AtspiSemanticDimension::StateSet)
            && observation.origin() != AtspiObservationOrigin::DirectReconciliation
        {
            return Err(AtspiActionEligibilityError::ReconciliationRequired);
        }
        if observation.states().contains(State::Defunct) {
            binding.invalidate_defunct();
            return Err(AtspiActionEligibilityError::Defunct);
        }

        Ok(AtspiActionEligibilityPermit::new_reconciled(
            binding,
            observation,
        ))
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

    fn authorize_pointer_from_observation(
        &self,
        binding: &AtspiElementBinding,
        states: StateSet,
        hit_test: AtspiPointerHitTest,
    ) -> Result<AtspiPointerEligibilityPermit, AtspiPointerEligibilityError> {
        self.authorize_from_state_set(binding, states)
            .map_err(AtspiPointerEligibilityError::Semantic)?;

        if !states.contains(State::Visible) {
            return Err(AtspiPointerEligibilityError::NotVisible);
        }
        if !states.contains(State::Showing) {
            return Err(AtspiPointerEligibilityError::NotShowing);
        }

        match hit_test {
            AtspiPointerHitTest::Unavailable => Err(AtspiPointerEligibilityError::HitTestUnavailable),
            AtspiPointerHitTest::Other(_) => Err(AtspiPointerEligibilityError::Occluded),
            AtspiPointerHitTest::Target(endpoint) if &endpoint == binding.endpoint() => {
                Ok(AtspiPointerEligibilityPermit::new(binding))
            }
            AtspiPointerHitTest::Target(_) => Err(AtspiPointerEligibilityError::Occluded),
        }
    }
}
