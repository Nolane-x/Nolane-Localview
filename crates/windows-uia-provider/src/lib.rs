#![cfg_attr(not(windows), forbid(unsafe_code))]

use std::{fmt, sync::Arc, time::Duration};

use localview_native_provider::{
    NativeProviderCapabilities, NativeProviderIdentityError, NativeSemanticSnapshotRevision,
    SnapshotBudget, SnapshotPublishError, UserSelectedWindowTarget, WindowsTargetFingerprint,
};
use localview_protocol::{ProviderElementRef, ProviderIncarnationRef, TargetIncarnationRef};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaWorkerConfig {
    pub snapshot_budget: SnapshotBudget,
    pub command_timeout: Duration,
}

impl Default for WindowsUiaWorkerConfig {
    fn default() -> Self {
        Self {
            snapshot_budget: SnapshotBudget::default(),
            command_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaSnapshotRequest {
    pub snapshot_cut_ref: String,
    pub surface_scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaElementLeaseRequest {
    pub snapshot_cut_ref: String,
    pub element_ref: ProviderElementRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaElementLeaseReceipt {
    pub snapshot_cut_ref: String,
    pub provider_incarnation_ref: ProviderIncarnationRef,
    pub target_incarnation_ref: TargetIncarnationRef,
    pub element_ref: ProviderElementRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUiaAttachment {
    selection: UserSelectedWindowTarget,
    provider_incarnation_ref: ProviderIncarnationRef,
    target_incarnation_ref: TargetIncarnationRef,
    fingerprint: WindowsTargetFingerprint,
}

impl WindowsUiaAttachment {
    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        &self.provider_incarnation_ref
    }

    pub fn target_incarnation_ref(&self) -> &TargetIncarnationRef {
        &self.target_incarnation_ref
    }

    pub fn fingerprint(&self) -> &WindowsTargetFingerprint {
        &self.fingerprint
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum WindowsUiaWorkerError {
    #[error("Windows UI Automation provider is unsupported on this platform")]
    UnsupportedPlatform,
    #[error("Windows UI Automation worker configuration is invalid")]
    InvalidConfiguration,
    #[error("Windows UI Automation worker could not start: {0}")]
    WorkerStartupFailed(String),
    #[error("Windows UI Automation worker is unavailable")]
    WorkerUnavailable,
    #[error("Windows UI Automation provider command timed out")]
    CommandTimeout,
    #[error("Windows UI Automation worker is poisoned after a command timeout")]
    WorkerPoisoned,
    #[error("Windows UI Automation target identity changed after attachment")]
    TargetReincarnated,
    #[error("Windows UI Automation snapshot request is invalid")]
    InvalidSnapshotRequest,
    #[error("Windows UI Automation element lease request is invalid")]
    InvalidElementLeaseRequest,
    #[error("Windows UI Automation dispatch context request is invalid")]
    InvalidDispatchContextRequest,
    #[error("Windows UI Automation pattern dispatch request is invalid")]
    InvalidPatternDispatchRequest,
    #[error("Windows UI Automation SetValue dispatch request is invalid")]
    InvalidSetValueDispatchRequest,
    #[error("Windows UI Automation SetValue verification request is invalid")]
    InvalidSetValueVerificationRequest,
    #[error("Windows UI Automation verified input request is invalid")]
    InvalidVerifiedInputRequest,
    #[error("Windows UI Automation verified input boundary failed: {0}")]
    VerifiedInputBoundary(#[from] crate::WindowsVerifiedInputBoundaryError),
    #[error("Windows UI Automation fresh SetValue element identity is ambiguous")]
    SetValueVerificationElementAmbiguous,
    #[error("Windows UI Automation SetValue password field is blocked")]
    SetValuePasswordFieldBlocked,
    #[error("Windows UI Automation SetValue password state is unavailable")]
    SetValuePasswordStateUnavailable,
    #[error("Windows UI Automation SetValue target is read-only")]
    SetValueReadOnly,
    #[error("Windows UI Automation SetValue read-only state is unavailable")]
    SetValueReadOnlyStateUnavailable,
    #[error("Windows UI Automation pattern is not enabled for real dispatch: {pattern:?}")]
    PatternDispatchUnsupported { pattern: crate::WindowsUiaPattern },
    #[error(
        "Windows UI Automation pattern is unavailable at the final dispatch boundary: {pattern:?}"
    )]
    PatternUnavailable { pattern: crate::WindowsUiaPattern },
    #[error(
        "Windows UI Automation element lease snapshot expired: requested {requested_cut}, current {current_cut}"
    )]
    ElementLeaseSnapshotExpired {
        requested_cut: String,
        current_cut: String,
    },
    #[error("Windows UI Automation exact element lease was not found in the latest snapshot")]
    ElementLeaseNotFound,
    #[error("Windows UI Automation virtualized-item request does not match worker authority")]
    InvalidVirtualizedItemRequest,
    #[error("Windows UI Automation ItemContainer pattern is unavailable")]
    VirtualizedItemContainerPatternUnavailable,
    #[error("Windows UI Automation VirtualizedItem pattern is unavailable")]
    VirtualizedItemPatternUnavailable,
    #[error("Windows UI Automation retained virtualized placeholder is unavailable")]
    VirtualizedItemPlaceholderNotFound,
    #[error("Windows UI Automation dispatch context is blocked: {0}")]
    DispatchContextBlocked(#[from] crate::WindowsUiaDispatchContextBlocker),
    #[error("Windows target identity error: {0}")]
    Identity(#[from] NativeProviderIdentityError),
    #[error("Windows UI Automation provider failure: {0}")]
    ProviderFailure(String),
    #[error("Windows semantic snapshot publication failed: {0}")]
    Snapshot(#[from] SnapshotPublishError),
}

#[cfg(windows)]
mod platform {
    use std::{
        collections::{BTreeMap, HashMap, VecDeque},
        ffi::c_void,
        sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
        thread,
    };

    use localview_live_bridge::CanonicalActionOperation;
    use localview_native_provider::{
        NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
        SnapshotBudgetGuard, derive_windows_target_incarnation,
        provider_element_ref_from_runtime_id,
    };
    use localview_protocol::{ProviderElementRealization, ReconciliationCompleteness};
    use uuid::Uuid;
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME, HWND},
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize, SAFEARRAY,
            },
            Ole::{
                SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound,
                SafeArrayGetUBound,
            },
            Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
            Variant::VARIANT,
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement,
                IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
                IUIAutomationItemContainerPattern, IUIAutomationSelectionItemPattern,
                IUIAutomationTogglePattern, IUIAutomationTreeWalker, IUIAutomationValuePattern,
                IUIAutomationVirtualizedItemPattern, UIA_AutomationIdPropertyId,
                UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
                UIA_IsExpandCollapsePatternAvailablePropertyId,
                UIA_IsInvokePatternAvailablePropertyId, UIA_IsScrollItemPatternAvailablePropertyId,
                UIA_IsSelectionItemPatternAvailablePropertyId,
                UIA_IsTogglePatternAvailablePropertyId, UIA_IsValuePatternAvailablePropertyId,
                UIA_IsVirtualizedItemPatternAvailablePropertyId, UIA_ItemContainerPatternId,
                UIA_NamePropertyId, UIA_PROPERTY_ID, UIA_SelectionItemIsSelectedPropertyId,
                UIA_SelectionItemPatternId, UIA_TogglePatternId, UIA_ToggleToggleStatePropertyId,
                UIA_ValuePatternId, UIA_VirtualizedItemPatternId,
            },
            WindowsAndMessaging::{
                GetForegroundWindow, GetLastActivePopup, GetWindowThreadProcessId, IsWindowVisible,
            },
        },
    };
    use windows::core::BSTR;

    use super::*;
    use crate::worker_health::{WorkerHealth, WorkerReceiveError};
    use crate::{
        WindowsInputDispatchBlocker, WindowsUiaActionCapabilities, WindowsUiaBooleanCapabilityFact,
        WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextReceipt,
        WindowsUiaDispatchContextRequest, WindowsUiaItemLookupProperty, WindowsUiaPattern,
        WindowsUiaPatternDispatchOperation, WindowsUiaPatternDispatchReceipt,
        WindowsUiaPatternDispatchRequest, WindowsUiaPatternSupport,
        WindowsUiaSetValueDispatchReceipt, WindowsUiaSetValueDispatchRequest,
        WindowsUiaSetValueEquality, WindowsUiaSetValueVerificationReceipt,
        WindowsUiaSetValueVerificationRequest, WindowsUiaValueCapabilityFacts,
        WindowsUiaVerifiedInputReceipt, WindowsUiaVerifiedInputRequest,
        WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaVirtualizedItemQueryRequest,
        WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaVirtualizedItemRealizeRequest,
        WindowsVerifiedInputBoundaryError, WindowsVerifiedInputBoundaryReceipt,
        classify_windows_input_insertion, evaluate_windows_keyboard_state,
        evaluate_windows_uia_dispatch_context, snapshot_windows_keyboard_state,
        windows_insert_verified_key_events,
    };

    const PROPERTIES_PER_NODE: usize = 19;
    const CACHE_PROFILE_REVISION: &str = "windows-uia-control-view-v1";
    const PERMISSION_VISIBILITY_REVISION: &str = "windows-uia-interactive-user-v1";
    const SELECTION_ITEM_IS_SELECTED_ATTRIBUTE: &str = "windows_uia.selection_item.is_selected";
    const TOGGLE_STATE_ATTRIBUTE: &str = "windows_uia.toggle.state";
    const EXPAND_COLLAPSE_STATE_ATTRIBUTE: &str = "windows_uia.expand_collapse.state";

    enum WorkerCommand {
        Attach {
            selection: UserSelectedWindowTarget,
            reply: Sender<Result<WindowsUiaAttachment, WindowsUiaWorkerError>>,
        },
        Snapshot {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaSnapshotRequest,
            reply: Sender<Result<Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError>>,
        },
        BindElementLease {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaElementLeaseRequest,
            reply: Sender<Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError>>,
        },
        RevalidateDispatchContext {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaDispatchContextRequest,
            reply: Sender<Result<WindowsUiaDispatchContextReceipt, WindowsUiaWorkerError>>,
        },
        DispatchPattern {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaPatternDispatchRequest,
            reply: Sender<Result<WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError>>,
        },
        DispatchSetValue {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaSetValueDispatchRequest,
            reply: Sender<Result<WindowsUiaSetValueDispatchReceipt, WindowsUiaWorkerError>>,
        },
        DispatchVerifiedInput {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaVerifiedInputRequest,
            reply: Sender<Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError>>,
        },
        VerifySetValue {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaSetValueVerificationRequest,
            reply: Sender<Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError>>,
        },
        QueryVirtualizedItem {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemQueryRequest,
            reply: Sender<Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError>>,
        },
        RealizeVirtualizedItem {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemRealizeRequest,
            reply: Sender<Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError>>,
        },
        Shutdown,
    }

    struct RetainedElementLease {
        element_ref: ProviderElementRef,
        element: IUIAutomationElement,
    }

    struct RetainedElementLeaseSet {
        snapshot_cut_ref: String,
        elements: Vec<RetainedElementLease>,
    }

    struct RetainedVirtualizedPlaceholder {
        element_ref: ProviderElementRef,
        element: IUIAutomationElement,
    }

    struct RetainedVirtualizedPlaceholderSet {
        snapshot_cut_ref: String,
        placeholders: Vec<RetainedVirtualizedPlaceholder>,
    }

    struct WorkerState {
        automation: IUIAutomation,
        walker: IUIAutomationTreeWalker,
        provider_incarnation_ref: ProviderIncarnationRef,
        snapshot_budget: SnapshotBudget,
        caches: HashMap<TargetIncarnationRef, SemanticSnapshotCache>,
        element_leases: HashMap<TargetIncarnationRef, RetainedElementLeaseSet>,
        virtualized_placeholders: HashMap<TargetIncarnationRef, RetainedVirtualizedPlaceholderSet>,
    }

    pub struct WindowsUiaWorker {
        sender: Sender<WorkerCommand>,
        command_timeout: Duration,
        provider_incarnation_ref: ProviderIncarnationRef,
        health: Arc<WorkerHealth>,
    }

    impl fmt::Debug for WindowsUiaWorker {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("WindowsUiaWorker")
                .field("provider_incarnation_ref", &self.provider_incarnation_ref)
                .field("command_timeout", &self.command_timeout)
                .finish_non_exhaustive()
        }
    }

    impl WindowsUiaWorker {
        pub fn capabilities() -> NativeProviderCapabilities {
            NativeProviderCapabilities::windows_observe_only()
        }

        pub fn spawn(config: WindowsUiaWorkerConfig) -> Result<Self, WindowsUiaWorkerError> {
            if config.command_timeout.is_zero() {
                return Err(WindowsUiaWorkerError::InvalidConfiguration);
            }

            let (command_tx, command_rx) = mpsc::channel();
            let (startup_tx, startup_rx) = mpsc::sync_channel(1);
            let snapshot_budget = config.snapshot_budget;
            thread::Builder::new()
                .name("localview-windows-uia-mta".into())
                .spawn(move || worker_main(command_rx, startup_tx, snapshot_budget))
                .map_err(|error| WindowsUiaWorkerError::WorkerStartupFailed(error.to_string()))?;

            let provider_incarnation_ref = match startup_rx.recv_timeout(config.command_timeout) {
                Ok(Ok(provider)) => provider,
                Ok(Err(error)) => return Err(error),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(WindowsUiaWorkerError::CommandTimeout);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(WindowsUiaWorkerError::WorkerUnavailable);
                }
            };

            Ok(Self {
                sender: command_tx,
                command_timeout: config.command_timeout,
                provider_incarnation_ref,
                health: Arc::new(WorkerHealth::new()),
            })
        }

        fn ensure_healthy(&self) -> Result<(), WindowsUiaWorkerError> {
            self.health
                .ensure_healthy()
                .map_err(|_| WindowsUiaWorkerError::WorkerPoisoned)
        }

        fn receive<T>(
            &self,
            receiver: &Receiver<Result<T, WindowsUiaWorkerError>>,
        ) -> Result<T, WindowsUiaWorkerError> {
            match self.health.recv_timeout(receiver, self.command_timeout) {
                Ok(result) => result,
                Err(WorkerReceiveError::Poisoned) => Err(WindowsUiaWorkerError::WorkerPoisoned),
                Err(WorkerReceiveError::Timeout) => Err(WindowsUiaWorkerError::CommandTimeout),
                Err(WorkerReceiveError::Disconnected) => {
                    Err(WindowsUiaWorkerError::WorkerUnavailable)
                }
            }
        }

        pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
            &self.provider_incarnation_ref
        }

        pub fn attach(
            &self,
            selection: UserSelectedWindowTarget,
        ) -> Result<WindowsUiaAttachment, WindowsUiaWorkerError> {
            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::Attach {
                    selection,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn snapshot(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSnapshotRequest,
        ) -> Result<Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError> {
            if request.snapshot_cut_ref.trim().is_empty() || request.surface_scope.trim().is_empty()
            {
                return Err(WindowsUiaWorkerError::InvalidSnapshotRequest);
            }
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::Snapshot {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn bind_element_lease(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaElementLeaseRequest,
        ) -> Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError> {
            if request.snapshot_cut_ref.trim().is_empty() {
                return Err(WindowsUiaWorkerError::InvalidElementLeaseRequest);
            }
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::BindElementLease {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn revalidate_dispatch_context(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaDispatchContextRequest,
        ) -> Result<WindowsUiaDispatchContextReceipt, WindowsUiaWorkerError> {
            if request.snapshot_cut_ref.trim().is_empty() {
                return Err(WindowsUiaWorkerError::InvalidDispatchContextRequest);
            }
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::RevalidateDispatchContext {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn dispatch_pattern(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaPatternDispatchRequest,
        ) -> Result<WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError> {
            if request.dispatch_attempt_ref.is_nil()
                || request.action_id.is_nil()
                || request.preparation_journal_sequence == 0
                || request.preparation_receipt_ref.trim().is_empty()
                || request.snapshot_cut_ref.trim().is_empty()
                || request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.dispatch_operation.required_pattern() != request.required_pattern
            {
                return Err(WindowsUiaWorkerError::InvalidPatternDispatchRequest);
            }
            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::DispatchPattern {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn dispatch_set_value(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSetValueDispatchRequest,
        ) -> Result<WindowsUiaSetValueDispatchReceipt, WindowsUiaWorkerError> {
            if request.dispatch_attempt_ref.is_nil()
                || request.action_id.is_nil()
                || request.preparation_journal_sequence == 0
                || request.preparation_receipt_ref.trim().is_empty()
                || request.snapshot_cut_ref.trim().is_empty()
                || request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref
                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref
                || request.element_ref.acquisition_cut_ref != request.snapshot_cut_ref
            {
                return Err(WindowsUiaWorkerError::InvalidSetValueDispatchRequest);
            }
            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::DispatchSetValue {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn dispatch_verified_input(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVerifiedInputRequest,
        ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> {
            if request.dispatch_attempt_ref.is_nil()
                || request.action_id.is_nil()
                || request.preparation_journal_sequence == 0
                || request.preparation_receipt_ref.trim().is_empty()
                || request.snapshot_cut_ref.trim().is_empty()
                || request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref
                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref
                || request.element_ref.acquisition_cut_ref != request.snapshot_cut_ref
            {
                return Err(WindowsUiaWorkerError::InvalidVerifiedInputRequest);
            }
            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::DispatchVerifiedInput {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub(crate) fn query_virtualized_item_on_mta(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemQueryRequest,
        ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> {
            let container = request.container_element_ref();
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref
                || container.provider_incarnation_ref != self.provider_incarnation_ref
                || container.target_incarnation_ref != attachment.target_incarnation_ref
                || container.acquisition_cut_ref != request.snapshot_cut_ref()
            {
                return Err(WindowsUiaWorkerError::InvalidVirtualizedItemRequest);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::QueryVirtualizedItem {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub(crate) fn realize_virtualized_item_on_mta(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemRealizeRequest,
        ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> {
            let placeholder = request.placeholder_element_ref();
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref
                || placeholder.provider_incarnation_ref != self.provider_incarnation_ref
                || placeholder.target_incarnation_ref != attachment.target_incarnation_ref
                || placeholder.acquisition_cut_ref != request.snapshot_cut_ref()
            {
                return Err(WindowsUiaWorkerError::InvalidVirtualizedItemRequest);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::RealizeVirtualizedItem {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }

        pub fn verify_set_value(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSetValueVerificationRequest,
        ) -> Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError> {
            if request.action_id.is_nil()
                || request.payload_ref.0.is_nil()
                || request.observation_cut_ref.trim().is_empty()
                || request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref
                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref
                || request.element_ref.acquisition_cut_ref == request.observation_cut_ref
            {
                return Err(WindowsUiaWorkerError::InvalidSetValueVerificationRequest);
            }
            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::VerifySetValue {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            self.receive(&reply_rx)
        }
    }

    impl Drop for WindowsUiaWorker {
        fn drop(&mut self) {
            // Never join here: a hostile or hung UIA provider must not freeze the
            // LocalView caller during cleanup. A responsive worker consumes this
            // shutdown command and uninitializes COM on its owning MTA thread.
            let _ = self.sender.send(WorkerCommand::Shutdown);
        }
    }

    fn worker_main(
        receiver: Receiver<WorkerCommand>,
        startup: mpsc::SyncSender<Result<ProviderIncarnationRef, WindowsUiaWorkerError>>,
        snapshot_budget: SnapshotBudget,
    ) {
        let initialized = unsafe {
            // SAFETY: This dedicated worker owns its COM apartment for its entire
            // lifetime and never exposes UIA COM interfaces to another thread.
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()
        };
        if let Err(error) = initialized {
            let _ = startup.send(Err(WindowsUiaWorkerError::WorkerStartupFailed(
                error.to_string(),
            )));
            return;
        }

        let automation = unsafe {
            // SAFETY: COM was initialized as MTA immediately above and the
            // returned IUIAutomation interface remains on this worker thread.
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        };
        let automation = match automation {
            Ok(automation) => automation,
            Err(error) => {
                let _ = startup.send(Err(WindowsUiaWorkerError::WorkerStartupFailed(
                    error.to_string(),
                )));
                unsafe {
                    // SAFETY: paired with successful CoInitializeEx on this thread.
                    CoUninitialize();
                }
                return;
            }
        };

        let walker = unsafe {
            // SAFETY: the UIA interface is live and remains owned by this MTA.
            automation.ControlViewWalker()
        };
        let walker = match walker {
            Ok(walker) => walker,
            Err(error) => {
                let _ = startup.send(Err(WindowsUiaWorkerError::WorkerStartupFailed(
                    error.to_string(),
                )));
                unsafe {
                    // SAFETY: paired with successful CoInitializeEx on this thread.
                    CoUninitialize();
                }
                return;
            }
        };

        let provider_incarnation_ref =
            ProviderIncarnationRef::from(format!("provider:windows-uia:mta:{}", Uuid::new_v4()));
        let mut state = WorkerState {
            automation,
            walker,
            provider_incarnation_ref: provider_incarnation_ref.clone(),
            snapshot_budget,
            caches: HashMap::new(),
            element_leases: HashMap::new(),
            virtualized_placeholders: HashMap::new(),
        };
        if startup.send(Ok(provider_incarnation_ref)).is_err() {
            unsafe {
                // SAFETY: paired with successful CoInitializeEx on this thread.
                CoUninitialize();
            }
            return;
        }

        while let Ok(command) = receiver.recv() {
            match command {
                WorkerCommand::Attach { selection, reply } => {
                    let _ = reply.send(state.attach(selection));
                }
                WorkerCommand::Snapshot {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.snapshot(&attachment, request));
                }
                WorkerCommand::BindElementLease {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.bind_element_lease(&attachment, request));
                }
                WorkerCommand::RevalidateDispatchContext {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.revalidate_dispatch_context(&attachment, request));
                }
                WorkerCommand::DispatchPattern {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.dispatch_pattern(&attachment, request));
                }
                WorkerCommand::DispatchSetValue {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.dispatch_set_value(&attachment, request));
                }
                WorkerCommand::DispatchVerifiedInput {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.dispatch_verified_input(&attachment, request));
                }
                WorkerCommand::VerifySetValue {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.verify_set_value(&attachment, request));
                }
                WorkerCommand::QueryVirtualizedItem {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.query_virtualized_item(&attachment, request));
                }
                WorkerCommand::RealizeVirtualizedItem {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.realize_virtualized_item(&attachment, request));
                }
                WorkerCommand::Shutdown => break,
            }
        }

        drop(state);
        unsafe {
            // SAFETY: all apartment-owned COM interfaces were dropped above and
            // this call is on the exact thread that initialized COM.
            CoUninitialize();
        }
    }

    impl WorkerState {
        fn attach(
            &self,
            selection: UserSelectedWindowTarget,
        ) -> Result<WindowsUiaAttachment, WindowsUiaWorkerError> {
            let fingerprint = self.fingerprint(&selection)?;
            let target_incarnation_ref =
                derive_windows_target_incarnation(&selection, &fingerprint)?;
            Ok(WindowsUiaAttachment {
                selection,
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref,
                fingerprint,
            })
        }

        fn snapshot(
            &mut self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSnapshotRequest,
        ) -> Result<Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError> {
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            self.require_current_target(attachment)?;

            let hwnd = hwnd_from_u64(attachment.selection.native_window_handle);
            let root = unsafe {
                // SAFETY: HWND identity was revalidated immediately above and the
                // UIA interface is used only inside its owning MTA apartment.
                self.automation.ElementFromHandle(hwnd)
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            let cache = self
                .caches
                .entry(attachment.target_incarnation_ref.clone())
                .or_insert_with(|| {
                    SemanticSnapshotCache::for_lineage(
                        self.provider_incarnation_ref.clone(),
                        attachment.target_incarnation_ref.clone(),
                    )
                });
            let capture_sequence = cache
                .current()
                .map_or(1, |revision| revision.capture_sequence().saturating_add(1));

            let (nodes, retained_elements, resource_usage, mut debt) = observe_bounded_tree(
                &self.automation,
                &self.walker,
                root,
                &self.provider_incarnation_ref,
                &attachment.target_incarnation_ref,
                &request.snapshot_cut_ref,
                &request.surface_scope,
                capture_sequence,
                self.snapshot_budget,
            );
            if nodes.is_empty() {
                debt.push("uia_snapshot_returned_no_semantic_nodes".into());
            }
            debt.sort();
            debt.dedup();
            let completeness = if resource_usage.incomplete || !debt.is_empty() {
                ReconciliationCompleteness::Incomplete
            } else {
                ReconciliationCompleteness::Established
            };

            let revision = cache
                .publish(NativeSemanticSnapshotDraft {
                    provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                    target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                    snapshot_cut_ref: request.snapshot_cut_ref,
                    surface_scope: request.surface_scope,
                    cache_profile_revision: CACHE_PROFILE_REVISION.into(),
                    permission_visibility_revision: PERMISSION_VISIBILITY_REVISION.into(),
                    capture_sequence,
                    nodes,
                    resource_usage,
                    completeness,
                    incompleteness_debt: debt,
                })
                .map_err(WindowsUiaWorkerError::from)?;

            self.element_leases.insert(
                attachment.target_incarnation_ref.clone(),
                RetainedElementLeaseSet {
                    snapshot_cut_ref: revision.snapshot_cut_ref().to_owned(),
                    elements: retained_elements,
                },
            );
            // Any successful observation cut supersedes temporary placeholder
            // retention. A realization receipt never promotes the old ref; only
            // nodes observed in this new cut may become RealizedCurrent.
            self.virtualized_placeholders
                .remove(&attachment.target_incarnation_ref);
            Ok(revision)
        }

        fn bind_element_lease(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaElementLeaseRequest,
        ) -> Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError> {
            let retained = self.exact_retained_element(
                attachment,
                &request.snapshot_cut_ref,
                &request.element_ref,
            )?;

            Ok(WindowsUiaElementLeaseReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                element_ref: retained.element_ref.clone(),
            })
        }

        fn query_virtualized_item(
            &mut self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemQueryRequest,
        ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> {
            let container = {
                let retained = self.exact_retained_element(
                    attachment,
                    request.snapshot_cut_ref(),
                    request.container_element_ref(),
                )?;
                retained.element.clone()
            };

            let item_container = unsafe {
                // SAFETY: the retained container and returned pattern stay on this
                // worker's owning MTA for the entire lookup.
                container.GetCurrentPatternAs::<IUIAutomationItemContainerPattern>(
                    UIA_ItemContainerPatternId,
                )
            }
            .map_err(|_| WindowsUiaWorkerError::VirtualizedItemContainerPatternUnavailable)?;

            let property_id = match request.property() {
                WindowsUiaItemLookupProperty::Name => UIA_NamePropertyId,
                WindowsUiaItemLookupProperty::AutomationId => UIA_AutomationIdPropertyId,
            };
            let lookup_value = VARIANT::from(request.value());
            let placeholder = unsafe {
                // SAFETY: start-after is intentionally null to search the entire
                // exact ItemContainer; the VARIANT is local and valid for this call.
                item_container.FindItemByProperty(
                    None::<&IUIAutomationElement>,
                    property_id,
                    &lookup_value,
                )
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            // Microsoft requires a virtualized placeholder to support this pattern;
            // do not probe arbitrary placeholder properties because they may return
            // UIA_E_ELEMENTNOTAVAILABLE before realization.
            unsafe {
                placeholder.GetCurrentPatternAs::<IUIAutomationVirtualizedItemPattern>(
                    UIA_VirtualizedItemPatternId,
                )
            }
            .map_err(|_| WindowsUiaWorkerError::VirtualizedItemPatternUnavailable)?;

            let runtime_id = unsafe {
                // SAFETY: best-effort identity hint read from the MTA-owned placeholder.
                runtime_id_hint(&placeholder)
            }
            .unwrap_or_default();
            let mut placeholder_element_ref = provider_element_ref_from_runtime_id(
                self.provider_incarnation_ref.clone(),
                attachment.target_incarnation_ref.clone(),
                &runtime_id,
                request.snapshot_cut_ref(),
                ProviderElementRealization::RealizationRequired,
            );
            if runtime_id.is_empty() {
                placeholder_element_ref.opaque_provider_element_id =
                    format!("uia-virtualized-placeholder:{}", Uuid::new_v4());
            }
            placeholder_element_ref.parent_surface_ref =
                request.container_element_ref().parent_surface_ref.clone();
            placeholder_element_ref
                .semantic_locator_hints
                .push(match request.property() {
                    WindowsUiaItemLookupProperty::Name => format!("name={}", request.value()),
                    WindowsUiaItemLookupProperty::AutomationId => {
                        format!("automation_id={}", request.value())
                    }
                });

            let receipt = WindowsUiaVirtualizedItemQueryReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                container_element_ref: request.container_element_ref().clone(),
                placeholder_element_ref: placeholder_element_ref.clone(),
            };
            self.virtualized_placeholders.insert(
                attachment.target_incarnation_ref.clone(),
                RetainedVirtualizedPlaceholderSet {
                    snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
                    placeholders: vec![RetainedVirtualizedPlaceholder {
                        element_ref: placeholder_element_ref,
                        element: placeholder,
                    }],
                },
            );
            Ok(receipt)
        }

        fn realize_virtualized_item(
            &mut self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVirtualizedItemRealizeRequest,
        ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> {
            self.require_current_target(attachment)?;
            let placeholder = {
                let retained = self
                    .virtualized_placeholders
                    .get(&attachment.target_incarnation_ref)
                    .ok_or(WindowsUiaWorkerError::VirtualizedItemPlaceholderNotFound)?;
                if retained.snapshot_cut_ref != request.snapshot_cut_ref() {
                    return Err(WindowsUiaWorkerError::ElementLeaseSnapshotExpired {
                        requested_cut: request.snapshot_cut_ref().to_owned(),
                        current_cut: retained.snapshot_cut_ref.clone(),
                    });
                }
                retained
                    .placeholders
                    .iter()
                    .find(|retained| &retained.element_ref == request.placeholder_element_ref())
                    .ok_or(WindowsUiaWorkerError::VirtualizedItemPlaceholderNotFound)?
                    .element
                    .clone()
            };

            let virtualized_item = unsafe {
                // SAFETY: placeholder and pattern remain on this worker's MTA.
                placeholder.GetCurrentPatternAs::<IUIAutomationVirtualizedItemPattern>(
                    UIA_VirtualizedItemPatternId,
                )
            }
            .map_err(|_| WindowsUiaWorkerError::VirtualizedItemPatternUnavailable)?;
            unsafe {
                // SAFETY: this is the single provider-side realization operation.
                virtualized_item.Realize()
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            self.virtualized_placeholders
                .remove(&attachment.target_incarnation_ref);
            Ok(WindowsUiaVirtualizedItemRealizeReceipt {
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                previous_placeholder_ref: request.placeholder_element_ref().clone(),
            })
        }

        fn revalidate_dispatch_context(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaDispatchContextRequest,
        ) -> Result<WindowsUiaDispatchContextReceipt, WindowsUiaWorkerError> {
            let retained = self.exact_retained_element(
                attachment,
                &request.snapshot_cut_ref,
                &request.element_ref,
            )?;

            let foreground_window = unsafe {
                // SAFETY: read-only Win32 query with no borrowed pointers.
                GetForegroundWindow()
            };
            let foreground_window_handle = hwnd_to_u64(foreground_window);
            let foreground_process_id = foreground_window_handle.and_then(|_| {
                let mut process_id = 0_u32;
                let thread_id = unsafe {
                    // SAFETY: process_id is valid writable storage and foreground
                    // HWND was returned by GetForegroundWindow immediately above.
                    GetWindowThreadProcessId(foreground_window, Some(&mut process_id))
                };
                (thread_id != 0 && process_id != 0).then_some(process_id)
            });

            let exact_element_focused = if request.requirements.require_exact_element_focus {
                unsafe {
                    // SAFETY: both the current focused element and retained action
                    // element remain inside this worker's owning MTA apartment.
                    self.automation
                        .GetFocusedElement()
                        .ok()
                        .and_then(|focused| {
                            self.automation
                                .CompareElements(&focused, &retained.element)
                                .ok()
                        })
                        .map(|same| same.as_bool())
                }
            } else {
                None
            };

            let target_hwnd = hwnd_from_u64(attachment.selection.native_window_handle);
            let modal_blocker_window_handle = if request.requirements.require_no_modal_blocker {
                let popup = unsafe {
                    // SAFETY: target HWND was revalidated by exact_retained_element.
                    GetLastActivePopup(target_hwnd)
                };
                let popup_handle = hwnd_to_u64(popup);
                match popup_handle {
                    Some(handle)
                        if handle != attachment.selection.native_window_handle
                            && unsafe { IsWindowVisible(popup) }.as_bool() =>
                    {
                        Some(handle)
                    }
                    _ => None,
                }
            } else {
                None
            };

            let observation = WindowsUiaDispatchContextObservation {
                target_window_handle: attachment.selection.native_window_handle,
                target_process_id: attachment.fingerprint.process_id,
                foreground_window_handle,
                foreground_process_id,
                exact_element_focused,
                modal_blocker_window_handle,
            };
            evaluate_windows_uia_dispatch_context(request.requirements, &observation)?;

            Ok(WindowsUiaDispatchContextReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                element_ref: retained.element_ref.clone(),
                observation,
            })
        }

        fn dispatch_verified_input(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaVerifiedInputRequest,
        ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> {
            if request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref
                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref
                || request.element_ref.acquisition_cut_ref != request.snapshot_cut_ref
            {
                return Err(WindowsUiaWorkerError::InvalidVerifiedInputRequest);
            }

            let context = self.revalidate_dispatch_context(
                attachment,
                WindowsUiaDispatchContextRequest {
                    snapshot_cut_ref: request.snapshot_cut_ref.clone(),
                    element_ref: request.element_ref.clone(),
                    requirements: request.context_requirements,
                },
            )?;
            let keyboard_state = snapshot_windows_keyboard_state()?;
            evaluate_windows_keyboard_state(&keyboard_state).map_err(|error| match error {
                WindowsInputDispatchBlocker::InputStateConflict => {
                    WindowsVerifiedInputBoundaryError::InputStateConflict
                }
                other => WindowsVerifiedInputBoundaryError::InvalidInsertionResult(other),
            })?;

            let raw = windows_insert_verified_key_events(request.batch.events());
            let expected = request.batch.len() as u32;
            if raw.requested_event_count != expected {
                return Err(WindowsVerifiedInputBoundaryError::RequestedCountMismatch {
                    expected,
                    reported: raw.requested_event_count,
                }
                .into());
            }
            let insertion_class =
                classify_windows_input_insertion(expected, raw.inserted_event_count)
                    .map_err(WindowsVerifiedInputBoundaryError::InvalidInsertionResult)?;

            let boundary = WindowsVerifiedInputBoundaryReceipt {
                dispatch_context: context.observation,
                keyboard_state,
                requested_event_count: expected,
                inserted_event_count: raw.inserted_event_count,
                insertion_class,
                raw_error_code: raw.raw_error_code,
                reconciliation_required: true,
            };
            Ok(WindowsUiaVerifiedInputReceipt::from_request(
                request, boundary,
            ))
        }

        fn dispatch_pattern(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaPatternDispatchRequest,
        ) -> Result<WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError> {
            if request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.dispatch_operation.required_pattern() != request.required_pattern
            {
                return Err(WindowsUiaWorkerError::InvalidPatternDispatchRequest);
            }
            let context = self.revalidate_dispatch_context(
                attachment,
                WindowsUiaDispatchContextRequest {
                    snapshot_cut_ref: request.snapshot_cut_ref.clone(),
                    element_ref: request.element_ref.clone(),
                    requirements: request.context_requirements,
                },
            )?;
            let retained = self.exact_retained_element(
                attachment,
                &request.snapshot_cut_ref,
                &request.element_ref,
            )?;
            match request.dispatch_operation {
                WindowsUiaPatternDispatchOperation::Invoke => {
                    if read_pattern_support(
                        &retained.element,
                        UIA_IsInvokePatternAvailablePropertyId,
                    ) != WindowsUiaPatternSupport::Supported
                    {
                        return Err(WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::Invoke,
                        });
                    }
                    let invoke = unsafe {
                        retained
                            .element
                            .GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId)
                    }
                    .map_err(|_| {
                        WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::Invoke,
                        }
                    })?;
                    unsafe { invoke.Invoke() }.map_err(|error| {
                        WindowsUiaWorkerError::ProviderFailure(error.to_string())
                    })?;
                }
                WindowsUiaPatternDispatchOperation::Select => {
                    if read_pattern_support(
                        &retained.element,
                        UIA_IsSelectionItemPatternAvailablePropertyId,
                    ) != WindowsUiaPatternSupport::Supported
                    {
                        return Err(WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::SelectionItem,
                        });
                    }
                    let selection_item = unsafe {
                        retained
                            .element
                            .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                                UIA_SelectionItemPatternId,
                            )
                    }
                    .map_err(|_| {
                        WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::SelectionItem,
                        }
                    })?;
                    unsafe { selection_item.Select() }.map_err(|error| {
                        WindowsUiaWorkerError::ProviderFailure(error.to_string())
                    })?;
                }
                WindowsUiaPatternDispatchOperation::Toggle => {
                    if read_pattern_support(
                        &retained.element,
                        UIA_IsTogglePatternAvailablePropertyId,
                    ) != WindowsUiaPatternSupport::Supported
                    {
                        return Err(WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::Toggle,
                        });
                    }
                    let toggle = unsafe {
                        retained
                            .element
                            .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                    }
                    .map_err(|_| {
                        WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::Toggle,
                        }
                    })?;
                    unsafe { toggle.Toggle() }.map_err(|error| {
                        WindowsUiaWorkerError::ProviderFailure(error.to_string())
                    })?;
                }
                WindowsUiaPatternDispatchOperation::Expand
                | WindowsUiaPatternDispatchOperation::Collapse => {
                    if read_pattern_support(
                        &retained.element,
                        UIA_IsExpandCollapsePatternAvailablePropertyId,
                    ) != WindowsUiaPatternSupport::Supported
                    {
                        return Err(WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::ExpandCollapse,
                        });
                    }
                    let expand_collapse = unsafe {
                        retained
                            .element
                            .GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(
                                UIA_ExpandCollapsePatternId,
                            )
                    }
                    .map_err(|_| {
                        WindowsUiaWorkerError::PatternUnavailable {
                            pattern: WindowsUiaPattern::ExpandCollapse,
                        }
                    })?;
                    let dispatch_result = unsafe {
                        match request.dispatch_operation {
                            WindowsUiaPatternDispatchOperation::Expand => expand_collapse.Expand(),
                            WindowsUiaPatternDispatchOperation::Collapse => {
                                expand_collapse.Collapse()
                            }
                            _ => unreachable!("ExpandCollapse dispatch arm is operation-exact"),
                        }
                    };
                    dispatch_result.map_err(|error| {
                        WindowsUiaWorkerError::ProviderFailure(error.to_string())
                    })?;
                }
            }
            Ok(WindowsUiaPatternDispatchReceipt {
                dispatch_attempt_ref: request.dispatch_attempt_ref,
                action_id: request.action_id,
                preparation_journal_sequence: request.preparation_journal_sequence,
                preparation_receipt_ref: request.preparation_receipt_ref,
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: request.provider_incarnation_ref,
                target_incarnation_ref: request.target_incarnation_ref,
                element_ref: request.element_ref,
                required_pattern: request.required_pattern,
                dispatch_operation: request.dispatch_operation,
                context_requirements: request.context_requirements,
                final_context: context.observation,
                transport_result: localview_protocol::TransportResult::DeliveredToExecutor,
                dispatch_result: localview_protocol::DispatchResult::DispatchedFull,
            })
        }

        fn dispatch_set_value(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSetValueDispatchRequest,
        ) -> Result<WindowsUiaSetValueDispatchReceipt, WindowsUiaWorkerError> {
            if request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref
                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref
                || request.element_ref.acquisition_cut_ref != request.snapshot_cut_ref
            {
                return Err(WindowsUiaWorkerError::InvalidSetValueDispatchRequest);
            }

            let retained = self.exact_retained_element(
                attachment,
                &request.snapshot_cut_ref,
                &request.element_ref,
            )?;
            if read_pattern_support(&retained.element, UIA_IsValuePatternAvailablePropertyId)
                != WindowsUiaPatternSupport::Supported
            {
                return Err(WindowsUiaWorkerError::PatternUnavailable {
                    pattern: WindowsUiaPattern::Value,
                });
            }

            let is_password = unsafe {
                // SAFETY: the exact retained element remains on this worker's MTA.
                retained.element.CurrentIsPassword()
            }
            .map_err(|_| WindowsUiaWorkerError::SetValuePasswordStateUnavailable)?
            .as_bool();
            if is_password {
                return Err(WindowsUiaWorkerError::SetValuePasswordFieldBlocked);
            }

            let value_pattern = unsafe {
                // SAFETY: the exact retained element and ValuePattern COM object remain
                // on this dedicated MTA and no interface crosses thread boundaries.
                retained
                    .element
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            }
            .map_err(|_| WindowsUiaWorkerError::PatternUnavailable {
                pattern: WindowsUiaPattern::Value,
            })?;
            let is_read_only = unsafe {
                // SAFETY: value_pattern is live and owned by this exact MTA worker.
                value_pattern.CurrentIsReadOnly()
            }
            .map_err(|_| WindowsUiaWorkerError::SetValueReadOnlyStateUnavailable)?
            .as_bool();
            if is_read_only {
                return Err(WindowsUiaWorkerError::SetValueReadOnly);
            }

            // Re-evaluate volatile foreground/focus/modal state last, immediately
            // adjacent to the provider mutation.
            let context = self.revalidate_dispatch_context(
                attachment,
                WindowsUiaDispatchContextRequest {
                    snapshot_cut_ref: request.snapshot_cut_ref.clone(),
                    element_ref: request.element_ref.clone(),
                    requirements: request.context_requirements,
                },
            )?;

            let secret = request
                .secret_utf8_str()
                .map_err(|_| WindowsUiaWorkerError::InvalidSetValueDispatchRequest)?;
            let value = BSTR::from(secret);
            unsafe {
                // SAFETY: this is the single provider mutation for this moved command.
                // The temporary BSTR is adjacent to the call and never logged/persisted.
                value_pattern.SetValue(&value)
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            Ok(WindowsUiaSetValueDispatchReceipt {
                dispatch_attempt_ref: request.dispatch_attempt_ref,
                action_id: request.action_id,
                preparation_journal_sequence: request.preparation_journal_sequence,
                preparation_receipt_ref: request.preparation_receipt_ref,
                snapshot_cut_ref: request.snapshot_cut_ref,
                provider_incarnation_ref: request.provider_incarnation_ref,
                target_incarnation_ref: request.target_incarnation_ref,
                element_ref: request.element_ref,
                required_pattern: WindowsUiaPattern::Value,
                dispatch_operation: CanonicalActionOperation::SetValue,
                payload_ref: request.payload_ref,
                mode: request.mode,
                context_requirements: request.context_requirements,
                final_context: context.observation,
                transport_result: localview_protocol::TransportResult::DeliveredToExecutor,
                dispatch_result: localview_protocol::DispatchResult::DispatchedFull,
            })
        }

        fn verify_set_value(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSetValueVerificationRequest,
        ) -> Result<WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError> {
            if request.provider_incarnation_ref != self.provider_incarnation_ref
                || request.provider_incarnation_ref != attachment.provider_incarnation_ref
                || request.target_incarnation_ref != attachment.target_incarnation_ref
                || request.element_ref.provider_incarnation_ref != request.provider_incarnation_ref
                || request.element_ref.target_incarnation_ref != request.target_incarnation_ref
                || request.element_ref.acquisition_cut_ref == request.observation_cut_ref
            {
                return Err(WindowsUiaWorkerError::InvalidSetValueVerificationRequest);
            }

            let retained = self.fresh_retained_element_for_verification(
                attachment,
                &request.observation_cut_ref,
                &request.element_ref,
            )?;
            if read_pattern_support(&retained.element, UIA_IsValuePatternAvailablePropertyId)
                != WindowsUiaPatternSupport::Supported
            {
                return Err(WindowsUiaWorkerError::PatternUnavailable {
                    pattern: WindowsUiaPattern::Value,
                });
            }

            let is_password = unsafe {
                // SAFETY: the fresh retained element remains on this worker's MTA.
                retained.element.CurrentIsPassword()
            }
            .map_err(|_| WindowsUiaWorkerError::SetValuePasswordStateUnavailable)?
            .as_bool();
            if is_password {
                return Err(WindowsUiaWorkerError::SetValuePasswordFieldBlocked);
            }

            let value_pattern = unsafe {
                // SAFETY: the fresh exact retained element and ValuePattern stay on this MTA.
                retained
                    .element
                    .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            }
            .map_err(|_| WindowsUiaWorkerError::PatternUnavailable {
                pattern: WindowsUiaPattern::Value,
            })?;
            let is_read_only = unsafe {
                // SAFETY: value_pattern is live and owned by this exact MTA worker.
                value_pattern.CurrentIsReadOnly()
            }
            .map_err(|_| WindowsUiaWorkerError::SetValueReadOnlyStateUnavailable)?
            .as_bool();
            if is_read_only {
                return Err(WindowsUiaWorkerError::SetValueReadOnly);
            }

            let expected = request
                .expected_utf8_str()
                .map_err(|_| WindowsUiaWorkerError::InvalidSetValueVerificationRequest)?;
            let equality = match unsafe {
                // SAFETY: this is a single fresh read from an MTA-owned ValuePattern.
                value_pattern.CurrentValue()
            } {
                Ok(current) => {
                    if current == expected {
                        WindowsUiaSetValueEquality::Match
                    } else {
                        WindowsUiaSetValueEquality::Mismatch
                    }
                }
                Err(_) => WindowsUiaSetValueEquality::Unknown,
            };

            Ok(WindowsUiaSetValueVerificationReceipt {
                action_id: request.action_id,
                payload_ref: request.payload_ref,
                mode: request.mode,
                provider_incarnation_ref: request.provider_incarnation_ref,
                target_incarnation_ref: request.target_incarnation_ref,
                element_ref: request.element_ref,
                observation_cut_ref: request.observation_cut_ref,
                equality,
            })
        }

        fn fresh_retained_element_for_verification<'a>(
            &'a self,
            attachment: &WindowsUiaAttachment,
            observation_cut_ref: &str,
            authoritative_element_ref: &ProviderElementRef,
        ) -> Result<&'a RetainedElementLease, WindowsUiaWorkerError> {
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }
            self.require_current_target(attachment)?;

            let lease_set = self
                .element_leases
                .get(&attachment.target_incarnation_ref)
                .ok_or(WindowsUiaWorkerError::ElementLeaseNotFound)?;
            if lease_set.snapshot_cut_ref != observation_cut_ref {
                return Err(WindowsUiaWorkerError::ElementLeaseSnapshotExpired {
                    requested_cut: observation_cut_ref.to_owned(),
                    current_cut: lease_set.snapshot_cut_ref.clone(),
                });
            }

            let mut matches = lease_set.elements.iter().filter(|retained| {
                retained.element_ref.provider_family == authoritative_element_ref.provider_family
                    && retained.element_ref.provider_incarnation_ref
                        == authoritative_element_ref.provider_incarnation_ref
                    && retained.element_ref.target_incarnation_ref
                        == authoritative_element_ref.target_incarnation_ref
                    && retained.element_ref.opaque_provider_element_id
                        == authoritative_element_ref.opaque_provider_element_id
                    && retained.element_ref.lifetime_profile_revision
                        == authoritative_element_ref.lifetime_profile_revision
            });
            let retained = matches
                .next()
                .ok_or(WindowsUiaWorkerError::ElementLeaseNotFound)?;
            if matches.next().is_some() {
                return Err(WindowsUiaWorkerError::SetValueVerificationElementAmbiguous);
            }
            Ok(retained)
        }

        fn exact_retained_element<'a>(
            &'a self,
            attachment: &WindowsUiaAttachment,
            snapshot_cut_ref: &str,
            element_ref: &ProviderElementRef,
        ) -> Result<&'a RetainedElementLease, WindowsUiaWorkerError> {
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }
            self.require_current_target(attachment)?;

            let lease_set = self
                .element_leases
                .get(&attachment.target_incarnation_ref)
                .ok_or(WindowsUiaWorkerError::ElementLeaseNotFound)?;
            if lease_set.snapshot_cut_ref != snapshot_cut_ref {
                return Err(WindowsUiaWorkerError::ElementLeaseSnapshotExpired {
                    requested_cut: snapshot_cut_ref.to_owned(),
                    current_cut: lease_set.snapshot_cut_ref.clone(),
                });
            }

            lease_set
                .elements
                .iter()
                .find(|retained| &retained.element_ref == element_ref)
                .ok_or(WindowsUiaWorkerError::ElementLeaseNotFound)
        }

        fn require_current_target(
            &self,
            attachment: &WindowsUiaAttachment,
        ) -> Result<(), WindowsUiaWorkerError> {
            let current_fingerprint = self.fingerprint(&attachment.selection)?;
            let current_target =
                derive_windows_target_incarnation(&attachment.selection, &current_fingerprint)?;
            if current_target != attachment.target_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }
            Ok(())
        }

        fn fingerprint(
            &self,
            selection: &UserSelectedWindowTarget,
        ) -> Result<WindowsTargetFingerprint, WindowsUiaWorkerError> {
            if selection.native_window_handle == 0
                || selection.expected_process_id == 0
                || selection.selection_nonce.is_nil()
            {
                return Err(NativeProviderIdentityError::InvalidSelection.into());
            }

            let hwnd = hwnd_from_u64(selection.native_window_handle);
            let mut process_id = 0_u32;
            let thread_id = unsafe {
                // SAFETY: `process_id` is valid writable storage and HWND is a
                // value supplied by the explicit selection, validated by Win32.
                GetWindowThreadProcessId(hwnd, Some(&mut process_id))
            };
            if thread_id == 0 || process_id == 0 {
                return Err(WindowsUiaWorkerError::ProviderFailure(
                    "selected HWND is no longer a live Win32 window".into(),
                ));
            }

            let process_start_time_ticks = process_start_time_ticks(process_id)?;
            let root_runtime_id_hint = unsafe {
                // SAFETY: the UIA object and element remain inside the owning MTA;
                // RuntimeId is copied into a Rust Vec and never used as durable identity.
                self.automation
                    .ElementFromHandle(hwnd)
                    .ok()
                    .and_then(|element| runtime_id_hint(&element))
                    .unwrap_or_default()
            };

            Ok(WindowsTargetFingerprint {
                native_window_handle: selection.native_window_handle,
                process_id,
                process_start_time_ticks,
                root_runtime_id_hint,
            })
        }
    }

    fn hwnd_from_u64(value: u64) -> HWND {
        HWND(value as usize as *mut c_void)
    }

    fn hwnd_to_u64(value: HWND) -> Option<u64> {
        (!value.0.is_null()).then_some(value.0 as usize as u64)
    }

    fn process_start_time_ticks(process_id: u32) -> Result<u64, WindowsUiaWorkerError> {
        let process = unsafe {
            // SAFETY: the requested access is read-only process lifetime metadata.
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)
        }
        .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = unsafe {
            // SAFETY: all FILETIME pointers are valid for the duration of the call
            // and `process` was opened successfully above.
            GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user)
        };
        let _ = unsafe {
            // SAFETY: `process` is an owned handle returned by OpenProcess.
            CloseHandle(process)
        };
        result.map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

        let ticks = ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
        if ticks == 0 {
            return Err(NativeProviderIdentityError::InvalidProcessLifetime.into());
        }
        Ok(ticks)
    }

    // These nine inputs are deliberately explicit correctness/authority facts:
    // automation/traversal object/root, provider+target lineage, observation cut/scope,
    // capture sequence, and resource budget. Hiding them in mutable context would
    // make accidental cross-lineage reuse easier at this OS boundary.
    #[allow(clippy::too_many_arguments)]
    fn observe_bounded_tree(
        automation: &IUIAutomation,
        walker: &IUIAutomationTreeWalker,
        root: IUIAutomationElement,
        provider_incarnation_ref: &ProviderIncarnationRef,
        target_incarnation_ref: &TargetIncarnationRef,
        snapshot_cut_ref: &str,
        surface_scope: &str,
        capture_sequence: u64,
        budget: SnapshotBudget,
    ) -> (
        Vec<NativeSemanticNodeObservation>,
        Vec<RetainedElementLease>,
        localview_native_provider::SnapshotResourceUsage,
        Vec<String>,
    ) {
        let mut guard = SnapshotBudgetGuard::new(budget);
        let mut nodes = Vec::new();
        let mut retained_elements = Vec::new();
        let mut debt = Vec::new();
        let mut queue = VecDeque::from([(root, None, 0_usize)]);
        let mut seen_runtime_ids = HashMap::<Vec<i32>, IUIAutomationElement>::new();

        while let Some((element, parent_index, depth)) = queue.pop_front() {
            let runtime_id = unsafe { runtime_id_hint(&element) }.unwrap_or_default();
            if !runtime_id.is_empty() {
                if let Some(previous) = seen_runtime_ids.get(&runtime_id) {
                    match unsafe {
                        // SAFETY: both UIA elements and the automation interface are
                        // owned by this dedicated MTA for the entire comparison.
                        automation.CompareElements(previous, &element)
                    } {
                        Ok(same) if same.as_bool() => {
                            // ControlView can surface the same exact element through
                            // an alias/cycle (for example an expanded Win32 ComboBox).
                            // Do not emit or traverse the duplicate appearance.
                            continue;
                        }
                        Ok(_) => {
                            debt.push("uia_runtime_id_collision_distinct_elements".into());
                        }
                        Err(_) => {
                            debt.push("uia_runtime_id_collision_compare_unavailable".into());
                        }
                    }
                } else {
                    seen_runtime_ids.insert(runtime_id.clone(), element.clone());
                }
            }

            if !guard.admit_node(depth, PROPERTIES_PER_NODE) {
                continue;
            }

            let index = nodes.len();
            let mut node_debt = Vec::new();
            let name = read_bstr(
                unsafe { element.CurrentName() },
                "uia_property_name_unavailable",
                &mut node_debt,
            );
            let role = read_bstr(
                unsafe { element.CurrentLocalizedControlType() },
                "uia_property_localized_control_type_unavailable",
                &mut node_debt,
            );
            let automation_id = read_bstr(
                unsafe { element.CurrentAutomationId() },
                "uia_property_automation_id_unavailable",
                &mut node_debt,
            );
            let class_name = read_bstr(
                unsafe { element.CurrentClassName() },
                "uia_property_class_name_unavailable",
                &mut node_debt,
            );
            let control_type = match unsafe { element.CurrentControlType() } {
                Ok(value) => Some(format!("uia_control_type:{}", value.0)),
                Err(_) => {
                    node_debt.push("uia_property_control_type_unavailable".into());
                    None
                }
            };
            let is_enabled = match unsafe { element.CurrentIsEnabled() } {
                Ok(value) => Some(value.as_bool()),
                Err(_) => {
                    node_debt.push("uia_property_is_enabled_unavailable".into());
                    None
                }
            };
            let is_offscreen = match unsafe { element.CurrentIsOffscreen() } {
                Ok(value) => Some(value.as_bool()),
                Err(_) => {
                    node_debt.push("uia_property_is_offscreen_unavailable".into());
                    None
                }
            };
            let action_capabilities = observe_action_capabilities(&element);
            let value_capability_facts = observe_value_capability_facts(
                &element,
                action_capabilities.support_for(WindowsUiaPattern::Value),
                &mut node_debt,
            );
            let selection_item_is_selected = if action_capabilities
                .support_for(WindowsUiaPattern::SelectionItem)
                == WindowsUiaPatternSupport::Supported
            {
                match unsafe {
                    element.GetCurrentPropertyValue(UIA_SelectionItemIsSelectedPropertyId)
                } {
                    Ok(value) => match bool::try_from(&value) {
                        Ok(selected) => Some(selected),
                        Err(_) => {
                            node_debt
                                .push("uia_property_selection_item_is_selected_unavailable".into());
                            None
                        }
                    },
                    Err(_) => {
                        node_debt
                            .push("uia_property_selection_item_is_selected_unavailable".into());
                        None
                    }
                }
            } else {
                None
            };
            let toggle_state = if action_capabilities.support_for(WindowsUiaPattern::Toggle)
                == WindowsUiaPatternSupport::Supported
            {
                match unsafe { element.GetCurrentPropertyValue(UIA_ToggleToggleStatePropertyId) } {
                    Ok(value) => match i32::try_from(&value) {
                        Ok(0) => Some("off"),
                        Ok(1) => Some("on"),
                        Ok(2) => Some("indeterminate"),
                        Ok(_) | Err(_) => {
                            node_debt.push("uia_property_toggle_state_unavailable".into());
                            None
                        }
                    },
                    Err(_) => {
                        node_debt.push("uia_property_toggle_state_unavailable".into());
                        None
                    }
                }
            } else {
                None
            };
            let expand_collapse_state = if action_capabilities
                .support_for(WindowsUiaPattern::ExpandCollapse)
                == WindowsUiaPatternSupport::Supported
            {
                match unsafe {
                    element.GetCurrentPropertyValue(
                        windows::Win32::UI::Accessibility::UIA_ExpandCollapseExpandCollapseStatePropertyId,
                    )
                } {
                    Ok(value) => match i32::try_from(&value) {
                        Ok(0) => Some("collapsed"),
                        Ok(1) => Some("expanded"),
                        Ok(2) => Some("partially_expanded"),
                        Ok(3) => Some("leaf_node"),
                        Ok(_) | Err(_) => {
                            node_debt.push("uia_property_expand_collapse_state_unavailable".into());
                            None
                        }
                    },
                    Err(_) => {
                        node_debt.push("uia_property_expand_collapse_state_unavailable".into());
                        None
                    }
                }
            } else {
                None
            };

            let mut element_ref = provider_element_ref_from_runtime_id(
                provider_incarnation_ref.clone(),
                target_incarnation_ref.clone(),
                &runtime_id,
                snapshot_cut_ref,
                ProviderElementRealization::RealizedCurrent,
            );
            if runtime_id.is_empty() {
                element_ref.opaque_provider_element_id =
                    format!("uia-snapshot:{capture_sequence}:node:{index}");
            }
            element_ref.parent_surface_ref = Some(surface_scope.to_owned());
            if let Some(value) = automation_id.as_deref().filter(|value| !value.is_empty()) {
                element_ref
                    .semantic_locator_hints
                    .push(format!("automation_id={value}"));
            }
            if let Some(value) = class_name.as_deref().filter(|value| !value.is_empty()) {
                element_ref
                    .semantic_locator_hints
                    .push(format!("class_name={value}"));
            }
            if let Some(value) = name.as_deref().filter(|value| !value.is_empty()) {
                element_ref
                    .semantic_locator_hints
                    .push(format!("name={value}"));
            }

            let mut attributes = BTreeMap::new();
            attributes.insert("provider".into(), "windows_uia".into());
            if !runtime_id.is_empty() {
                attributes.insert("runtime_id_observed".into(), "true".into());
            }
            action_capabilities.write_attributes(&mut attributes);
            value_capability_facts.write_attributes(&mut attributes);
            if let Some(selected) = selection_item_is_selected {
                attributes.insert(
                    SELECTION_ITEM_IS_SELECTED_ATTRIBUTE.into(),
                    selected.to_string(),
                );
            }
            if let Some(state) = toggle_state {
                attributes.insert(TOGGLE_STATE_ATTRIBUTE.into(), state.into());
            }
            if let Some(state) = expand_collapse_state {
                attributes.insert(EXPAND_COLLAPSE_STATE_ATTRIBUTE.into(), state.into());
            }
            retained_elements.push(RetainedElementLease {
                element_ref: element_ref.clone(),
                element: element.clone(),
            });
            nodes.push(NativeSemanticNodeObservation {
                element_ref,
                parent_index,
                depth,
                role,
                name,
                control_type,
                automation_id,
                class_name,
                is_enabled,
                is_offscreen,
                attributes,
            });
            debt.extend(node_debt);

            if depth >= budget.max_depth || nodes.len() >= budget.max_nodes {
                continue;
            }

            let mut child = unsafe {
                // SAFETY: walker and element are apartment-owned COM interfaces.
                walker.GetFirstChildElement(&element)
            }
            .ok();
            while let Some(current_child) = child {
                if nodes.len().saturating_add(queue.len()) >= budget.max_nodes {
                    break;
                }
                let next = unsafe {
                    // SAFETY: current_child remains live in this MTA while asking
                    // the same walker for its next sibling.
                    walker.GetNextSiblingElement(&current_child)
                }
                .ok();
                queue.push_back((current_child, Some(index), depth.saturating_add(1)));
                child = next;
            }
        }

        let usage = guard.finish();
        if usage.incomplete {
            for limit in &usage.exhausted {
                debt.push(format!("snapshot_budget_exhausted:{limit:?}"));
            }
        }
        (nodes, retained_elements, usage, debt)
    }

    fn observe_value_capability_facts(
        element: &IUIAutomationElement,
        value_support: WindowsUiaPatternSupport,
        node_debt: &mut Vec<String>,
    ) -> WindowsUiaValueCapabilityFacts {
        let is_password = match unsafe {
            // SAFETY: the UIA element is retained and read only on this dedicated MTA.
            element.CurrentIsPassword()
        } {
            Ok(value) => WindowsUiaBooleanCapabilityFact::from_bool(value.as_bool()),
            Err(_) => {
                node_debt.push("uia_property_is_password_unavailable".into());
                WindowsUiaBooleanCapabilityFact::Unknown
            }
        };

        let is_read_only = if value_support == WindowsUiaPatternSupport::Supported {
            match unsafe {
                // SAFETY: the exact UIA element and temporary ValuePattern remain
                // apartment-owned; this reads capability state and performs no mutation.
                element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            } {
                Ok(pattern) => match unsafe { pattern.CurrentIsReadOnly() } {
                    Ok(value) => WindowsUiaBooleanCapabilityFact::from_bool(value.as_bool()),
                    Err(_) => {
                        node_debt.push("uia_property_value_is_read_only_unavailable".into());
                        WindowsUiaBooleanCapabilityFact::Unknown
                    }
                },
                Err(_) => {
                    node_debt.push("uia_property_value_is_read_only_unavailable".into());
                    WindowsUiaBooleanCapabilityFact::Unknown
                }
            }
        } else {
            WindowsUiaBooleanCapabilityFact::Unknown
        };

        WindowsUiaValueCapabilityFacts::new(value_support, is_password, is_read_only)
    }

    fn observe_action_capabilities(element: &IUIAutomationElement) -> WindowsUiaActionCapabilities {
        let mut capabilities = WindowsUiaActionCapabilities::default();
        for (pattern, property_id) in [
            (
                WindowsUiaPattern::Invoke,
                UIA_IsInvokePatternAvailablePropertyId,
            ),
            (
                WindowsUiaPattern::SelectionItem,
                UIA_IsSelectionItemPatternAvailablePropertyId,
            ),
            (
                WindowsUiaPattern::Value,
                UIA_IsValuePatternAvailablePropertyId,
            ),
            (
                WindowsUiaPattern::Toggle,
                UIA_IsTogglePatternAvailablePropertyId,
            ),
            (
                WindowsUiaPattern::ExpandCollapse,
                UIA_IsExpandCollapsePatternAvailablePropertyId,
            ),
            (
                WindowsUiaPattern::ScrollItem,
                UIA_IsScrollItemPatternAvailablePropertyId,
            ),
            (
                WindowsUiaPattern::VirtualizedItem,
                UIA_IsVirtualizedItemPatternAvailablePropertyId,
            ),
        ] {
            capabilities.record(pattern, read_pattern_support(element, property_id));
        }
        capabilities
    }

    fn read_pattern_support(
        element: &IUIAutomationElement,
        property_id: UIA_PROPERTY_ID,
    ) -> WindowsUiaPatternSupport {
        unsafe {
            // SAFETY: the UIA element remains owned by this dedicated MTA. The
            // returned VARIANT is converted immediately into a Rust bool and no
            // pattern COM object escapes or is invoked.
            element.GetCurrentPropertyValue(property_id)
        }
        .ok()
        .and_then(|value| bool::try_from(&value).ok())
        .map(|available| {
            if available {
                WindowsUiaPatternSupport::Supported
            } else {
                WindowsUiaPatternSupport::Unsupported
            }
        })
        .unwrap_or(WindowsUiaPatternSupport::Unknown)
    }

    fn read_bstr(
        result: windows::core::Result<windows::core::BSTR>,
        debt: &'static str,
        debts: &mut Vec<String>,
    ) -> Option<String> {
        match result {
            Ok(value) => Some(value.to_string()),
            Err(_) => {
                debts.push(debt.into());
                None
            }
        }
    }

    unsafe fn runtime_id_hint(element: &IUIAutomationElement) -> Option<Vec<i32>> {
        // SAFETY: caller guarantees `element` is apartment-owned and live. The
        // SAFEARRAY returned by UIA is copied element-by-element then destroyed.
        let array = unsafe { element.GetRuntimeId().ok()? };
        if array.is_null() {
            return None;
        }
        let guard = SafeArrayGuard(array);
        if unsafe { SafeArrayGetDim(guard.0) } != 1 {
            return None;
        }
        let lower = unsafe { SafeArrayGetLBound(guard.0, 1).ok()? };
        let upper = unsafe { SafeArrayGetUBound(guard.0, 1).ok()? };
        if upper < lower || (upper - lower) > 256 {
            return None;
        }

        let mut values = Vec::with_capacity((upper - lower + 1) as usize);
        for index in lower..=upper {
            let mut value = 0_i32;
            if unsafe {
                SafeArrayGetElement(guard.0, &index, (&mut value as *mut i32).cast::<c_void>())
            }
            .is_err()
            {
                return None;
            }
            values.push(value);
        }
        Some(values)
    }

    struct SafeArrayGuard(*mut SAFEARRAY);

    impl Drop for SafeArrayGuard {
        fn drop(&mut self) {
            let _ = unsafe {
                // SAFETY: this guard owns the SAFEARRAY returned by GetRuntimeId
                // and destroys it exactly once on the same worker thread.
                SafeArrayDestroy(self.0)
            };
        }
    }

    pub use WindowsUiaWorker as ExportedWindowsUiaWorker;
}

#[cfg(windows)]
pub use platform::ExportedWindowsUiaWorker as WindowsUiaWorker;

#[cfg(not(windows))]
pub struct WindowsUiaWorker;

#[cfg(not(windows))]
impl fmt::Debug for WindowsUiaWorker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WindowsUiaWorker(unsupported)")
    }
}

#[cfg(not(windows))]
impl WindowsUiaWorker {
    pub fn capabilities() -> NativeProviderCapabilities {
        NativeProviderCapabilities::windows_observe_only()
    }

    pub fn spawn(_config: WindowsUiaWorkerConfig) -> Result<Self, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
        unreachable!("Windows UIA worker cannot exist on this platform")
    }

    pub fn attach(
        &self,
        _selection: UserSelectedWindowTarget,
    ) -> Result<WindowsUiaAttachment, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn snapshot(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: WindowsUiaSnapshotRequest,
    ) -> Result<Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn bind_element_lease(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: WindowsUiaElementLeaseRequest,
    ) -> Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn revalidate_dispatch_context(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaDispatchContextRequest,
    ) -> Result<crate::WindowsUiaDispatchContextReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn dispatch_pattern(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaPatternDispatchRequest,
    ) -> Result<crate::WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn dispatch_set_value(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaSetValueDispatchRequest,
    ) -> Result<crate::WindowsUiaSetValueDispatchReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn verify_set_value(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaSetValueVerificationRequest,
    ) -> Result<crate::WindowsUiaSetValueVerificationReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }
}
