use localview_protocol::{
    ProviderElementRealization, ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef,
};
use thiserror::Error;

/// Bounded UIA property predicates that may be used for an ItemContainer lookup.
///
/// This deliberately excludes arbitrary UIA property IDs: the trusted runtime
/// chooses a small, auditable lookup vocabulary and the provider translates it
/// to native UIA identifiers inside the owning MTA worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUiaItemLookupProperty {
    Name,
    AutomationId,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsUiaVirtualizedItemRequestError {
    #[error("Windows UIA virtualized-item request requires a nonempty snapshot cut")]
    EmptySnapshotCut,
    #[error("Windows UIA virtualized-item lookup requires a nonempty value")]
    EmptyLookupValue,
    #[error("Windows UIA virtualized-item element acquisition cut does not match the request cut")]
    AcquisitionCutMismatch,
    #[error("Windows UIA ItemContainer query requires a currently realized container")]
    ContainerNotRealized,
    #[error("Windows UIA realization request requires a RealizationRequired placeholder")]
    PlaceholderNotRealizationRequired,
}

/// Exact, side-effect-free request to locate one provider-virtualized item from
/// a currently realized ItemContainer at one snapshot cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVirtualizedItemQueryRequest {
    snapshot_cut_ref: String,
    container_element_ref: ProviderElementRef,
    property: WindowsUiaItemLookupProperty,
    value: String,
}

impl WindowsUiaVirtualizedItemQueryRequest {
    pub fn new(
        snapshot_cut_ref: impl Into<String>,
        container_element_ref: ProviderElementRef,
        property: WindowsUiaItemLookupProperty,
        value: impl Into<String>,
    ) -> Result<Self, WindowsUiaVirtualizedItemRequestError> {
        let snapshot_cut_ref = snapshot_cut_ref.into();
        let value = value.into();
        if snapshot_cut_ref.trim().is_empty() {
            return Err(WindowsUiaVirtualizedItemRequestError::EmptySnapshotCut);
        }
        if value.trim().is_empty() {
            return Err(WindowsUiaVirtualizedItemRequestError::EmptyLookupValue);
        }
        if container_element_ref.acquisition_cut_ref != snapshot_cut_ref {
            return Err(WindowsUiaVirtualizedItemRequestError::AcquisitionCutMismatch);
        }
        if container_element_ref.realization != ProviderElementRealization::RealizedCurrent {
            return Err(WindowsUiaVirtualizedItemRequestError::ContainerNotRealized);
        }

        Ok(Self {
            snapshot_cut_ref,
            container_element_ref,
            property,
            value,
        })
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn container_element_ref(&self) -> &ProviderElementRef {
        &self.container_element_ref
    }

    pub fn property(&self) -> WindowsUiaItemLookupProperty {
        self.property
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Data-only evidence returned after the worker located and retained a real UIA
/// virtualized placeholder. The placeholder remains non-actionable until a
/// separate realization command followed by a fresh observation cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVirtualizedItemQueryReceipt {
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub container_element_ref: ProviderElementRef,
    pub placeholder_element_ref: ProviderElementRef,
}

/// Exact request to invoke VirtualizedItem::Realize on a worker-retained
/// placeholder. A successful call does not promote this stale placeholder ref to
/// RealizedCurrent; callers must obtain a fresh snapshot cut afterward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVirtualizedItemRealizeRequest {
    snapshot_cut_ref: String,
    placeholder_element_ref: ProviderElementRef,
}

impl WindowsUiaVirtualizedItemRealizeRequest {
    pub fn new(
        snapshot_cut_ref: impl Into<String>,
        placeholder_element_ref: ProviderElementRef,
    ) -> Result<Self, WindowsUiaVirtualizedItemRequestError> {
        let snapshot_cut_ref = snapshot_cut_ref.into();
        if snapshot_cut_ref.trim().is_empty() {
            return Err(WindowsUiaVirtualizedItemRequestError::EmptySnapshotCut);
        }
        if placeholder_element_ref.acquisition_cut_ref != snapshot_cut_ref {
            return Err(WindowsUiaVirtualizedItemRequestError::AcquisitionCutMismatch);
        }
        if placeholder_element_ref.realization
            != ProviderElementRealization::RealizationRequired
        {
            return Err(
                WindowsUiaVirtualizedItemRequestError::PlaceholderNotRealizationRequired,
            );
        }

        Ok(Self {
            snapshot_cut_ref,
            placeholder_element_ref,
        })
    }

    pub fn snapshot_cut_ref(&self) -> &str {
        &self.snapshot_cut_ref
    }

    pub fn placeholder_element_ref(&self) -> &ProviderElementRef {
        &self.placeholder_element_ref
    }
}

/// Provider-owned evidence that one exact retained placeholder received a
/// VirtualizedItem::Realize call. The previous ref remains explicitly present so
/// no caller can confuse the command receipt with fresh actionable identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaVirtualizedItemRealizeReceipt {
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub previous_placeholder_ref: ProviderElementRef,
}

impl crate::WindowsUiaWorker {
    /// Public fail-closed surface for W03. The real UIA implementation is added
    /// only after a hosted-Windows behavior test proves the missing provider
    /// primitive; until then this method must never mint placeholder authority.
    pub fn query_virtualized_item(
        &self,
        _attachment: &crate::worker::WindowsUiaAttachment,
        _request: WindowsUiaVirtualizedItemQueryRequest,
    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, crate::worker::WindowsUiaWorkerError> {
        Err(crate::worker::WindowsUiaWorkerError::ProviderFailure(
            "Windows UIA virtualized-item query is not implemented".into(),
        ))
    }

    /// Public fail-closed surface for W03. A realization receipt is intentionally
    /// impossible until the owning MTA has a tested VirtualizedItem implementation.
    pub fn realize_virtualized_item(
        &self,
        _attachment: &crate::worker::WindowsUiaAttachment,
        _request: WindowsUiaVirtualizedItemRealizeRequest,
    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, crate::worker::WindowsUiaWorkerError> {
        Err(crate::worker::WindowsUiaWorkerError::ProviderFailure(
            "Windows UIA virtualized-item realization is not implemented".into(),
        ))
    }
}
