#![cfg_attr(not(windows), forbid(unsafe_code))]

use std::{fmt, sync::Arc, time::Duration};

use localview_native_provider::{
    NativeProviderCapabilities, NativeProviderIdentityError, NativeSemanticSnapshotRevision,
    SnapshotBudget, SnapshotPublishError, UserSelectedWindowTarget, WindowsTargetFingerprint,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
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
    #[error("Windows UI Automation target identity changed after attachment")]
    TargetReincarnated,
    #[error("Windows UI Automation snapshot request is invalid")]
    InvalidSnapshotRequest,
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

    use localview_native_provider::{
        derive_windows_target_incarnation, provider_element_ref_from_runtime_id,
        NativeSemanticNodeObservation, NativeSemanticSnapshotDraft, SemanticSnapshotCache,
        SnapshotBudgetGuard,
    };
    use localview_protocol::{ProviderElementRealization, ReconciliationCompleteness};
    use uuid::Uuid;
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME, HWND},
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED, SAFEARRAY,
            },
            Ole::{
                SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetLBound,
                SafeArrayGetUBound,
            },
            Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
            },
            WindowsAndMessaging::GetWindowThreadProcessId,
        },
    };

    use super::*;

    const PROPERTIES_PER_NODE: usize = 7;
    const CACHE_PROFILE_REVISION: &str = "windows-uia-control-view-v1";
    const PERMISSION_VISIBILITY_REVISION: &str = "windows-uia-interactive-user-v1";

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
        Shutdown,
    }

    struct WorkerState {
        automation: IUIAutomation,
        walker: IUIAutomationTreeWalker,
        provider_incarnation_ref: ProviderIncarnationRef,
        snapshot_budget: SnapshotBudget,
        caches: HashMap<TargetIncarnationRef, SemanticSnapshotCache>,
    }

    pub struct WindowsUiaWorker {
        sender: Sender<WorkerCommand>,
        command_timeout: Duration,
        provider_incarnation_ref: ProviderIncarnationRef,
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
                Err(RecvTimeoutError::Timeout) => return Err(WindowsUiaWorkerError::CommandTimeout),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(WindowsUiaWorkerError::WorkerUnavailable)
                }
            };

            Ok(Self {
                sender: command_tx,
                command_timeout: config.command_timeout,
                provider_incarnation_ref,
            })
        }

        pub fn provider_incarnation_ref(&self) -> &ProviderIncarnationRef {
            &self.provider_incarnation_ref
        }

        pub fn attach(
            &self,
            selection: UserSelectedWindowTarget,
        ) -> Result<WindowsUiaAttachment, WindowsUiaWorkerError> {
            let (reply_tx, reply_rx) = mpsc::channel();
            self.sender
                .send(WorkerCommand::Attach {
                    selection,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_command(reply_rx, self.command_timeout)
        }

        pub fn snapshot(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaSnapshotRequest,
        ) -> Result<Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError> {
            if request.snapshot_cut_ref.trim().is_empty() || request.surface_scope.trim().is_empty() {
                return Err(WindowsUiaWorkerError::InvalidSnapshotRequest);
            }
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.sender
                .send(WorkerCommand::Snapshot {
                    attachment: attachment.clone(),
                    request,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_command(reply_rx, self.command_timeout)
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

    fn recv_command<T>(
        receiver: Receiver<Result<T, WindowsUiaWorkerError>>,
        timeout: Duration,
    ) -> Result<T, WindowsUiaWorkerError> {
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(WindowsUiaWorkerError::CommandTimeout),
            Err(RecvTimeoutError::Disconnected) => Err(WindowsUiaWorkerError::WorkerUnavailable),
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

        let provider_incarnation_ref = ProviderIncarnationRef::from(format!(
            "provider:windows-uia:mta:{}",
            Uuid::new_v4()
        ));
        let mut state = WorkerState {
            automation,
            walker,
            provider_incarnation_ref: provider_incarnation_ref.clone(),
            snapshot_budget,
            caches: HashMap::new(),
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

            let current_fingerprint = self.fingerprint(&attachment.selection)?;
            let current_target =
                derive_windows_target_incarnation(&attachment.selection, &current_fingerprint)?;
            if current_target != attachment.target_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

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

            let (nodes, resource_usage, mut debt) = observe_bounded_tree(
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

            cache
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
                .map_err(WindowsUiaWorkerError::from)
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
            GetProcessTimes(
                process,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
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

    fn observe_bounded_tree(
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
        localview_native_provider::SnapshotResourceUsage,
        Vec<String>,
    ) {
        let mut guard = SnapshotBudgetGuard::new(budget);
        let mut nodes = Vec::new();
        let mut debt = Vec::new();
        let mut queue = VecDeque::from([(root, None, 0_usize)]);

        while let Some((element, parent_index, depth)) = queue.pop_front() {
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

            let runtime_id = unsafe { runtime_id_hint(&element) }.unwrap_or_default();
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
        (nodes, usage, debt)
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
                SafeArrayGetElement(
                    guard.0,
                    &index,
                    (&mut value as *mut i32).cast::<c_void>(),
                )
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

    pub(super) use WindowsUiaWorker as ExportedWindowsUiaWorker;
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
}
