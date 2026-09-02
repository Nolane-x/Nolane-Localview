use std::{fmt, sync::Arc};

use localview_native_provider::{
    NativeProviderCapabilities, NativeSemanticSnapshotRevision, ProviderEventReliabilityProfile,
    UserSelectedWindowTarget,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
use uuid::Uuid;

use crate::{
    worker::{
        WindowsUiaAttachment, WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest,
        WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
    },
    WindowsUiaDispatchContextReceipt, WindowsUiaDispatchContextRequest, WindowsUiaEventDrain,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsUiaEventSubscriptionOptions {
    pub capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaEventSubscription {
    id: Uuid,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    sequence_baseline: u64,
    reliability_profile: ProviderEventReliabilityProfile,
}

impl WindowsUiaEventSubscription {
    pub fn sequence_baseline(&self) -> u64 {
        self.sequence_baseline
    }

    pub fn reliability_profile(&self) -> &ProviderEventReliabilityProfile {
        &self.reliability_profile
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }
}

pub struct WindowsUiaWorker {
    inner: crate::worker::WindowsUiaWorker,
}

impl fmt::Debug for WindowsUiaWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WindowsUiaWorker(unsupported)")
    }
}

impl WindowsUiaWorker {
    pub fn capabilities() -> NativeProviderCapabilities {
        crate::worker::WindowsUiaWorker::capabilities()
    }

    pub fn spawn(config: WindowsUiaWorkerConfig) -> Result<Self, WindowsUiaWorkerError> {
        crate::worker::WindowsUiaWorker::spawn(config).map(|inner| Self { inner })
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        self.inner.provider_incarnation_ref()
    }

    pub fn attach(
        &self,
        selection: UserSelectedWindowTarget,
    ) -> Result<WindowsUiaAttachment, WindowsUiaWorkerError> {
        self.inner.attach(selection)
    }

    pub fn snapshot(
        &self,
        attachment: &WindowsUiaAttachment,
        request: WindowsUiaSnapshotRequest,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError> {
        self.inner.snapshot(attachment, request)
    }

    pub fn bind_element_lease(
        &self,
        attachment: &WindowsUiaAttachment,
        request: WindowsUiaElementLeaseRequest,
    ) -> Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError> {
        self.inner.bind_element_lease(attachment, request)
    }

    pub fn revalidate_dispatch_context(
        &self,
        attachment: &WindowsUiaAttachment,
        request: WindowsUiaDispatchContextRequest,
    ) -> Result<WindowsUiaDispatchContextReceipt, WindowsUiaWorkerError> {
        self.inner.revalidate_dispatch_context(attachment, request)
    }

    pub fn subscribe_events(
        &self,
        _attachment: &WindowsUiaAttachment,
        _options: WindowsUiaEventSubscriptionOptions,
    ) -> Result<WindowsUiaEventSubscription, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn drain_events(
        &self,
        _subscription: &WindowsUiaEventSubscription,
        _limit: usize,
    ) -> Result<WindowsUiaEventDrain, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn unsubscribe_events(
        &self,
        _subscription: WindowsUiaEventSubscription,
    ) -> Result<(), WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }
}
