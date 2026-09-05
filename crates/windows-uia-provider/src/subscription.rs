use std::{fmt, time::Duration};

use localview_native_provider::{
    NativeProviderCapabilities, NativeSemanticSnapshotRevision, ProviderEventReliabilityProfile,
    UserSelectedWindowTarget,
};
use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
use uuid::Uuid;

use crate::{
    WindowsUiaBoundDispatchContextReceipt, WindowsUiaDispatchContextRequest, WindowsUiaEventDrain,
    WindowsUiaPatternDispatchReceipt, WindowsUiaPatternDispatchRequest,
    event_buffer::{WindowsUiaEventBuffer, WindowsUiaEventDraft, WindowsUiaEventKind},
    worker::{
        WindowsUiaAttachment, WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest,
        WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
    },
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

#[cfg(windows)]
mod platform {
    use std::{
        collections::HashMap,
        ffi::c_void,
        sync::{
            Arc, Mutex,
            mpsc::{self, Receiver, RecvTimeoutError, Sender},
        },
        thread,
    };

    use windows::{
        Win32::{
            Foundation::{CloseHandle, FILETIME, HWND},
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                    CoUninitialize,
                },
                Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
                Variant::VARIANT,
            },
            UI::{
                Accessibility::{
                    CUIAutomation, IUIAutomation, IUIAutomationElement,
                    IUIAutomationPropertyChangedEventHandler,
                    IUIAutomationPropertyChangedEventHandler_Impl, TreeScope_Subtree,
                    UIA_NamePropertyId, UIA_PROPERTY_ID,
                },
                WindowsAndMessaging::GetWindowThreadProcessId,
            },
        },
        core::Ref,
    };

    use super::*;

    enum EventWorkerCommand {
        Subscribe {
            attachment: WindowsUiaAttachment,
            options: WindowsUiaEventSubscriptionOptions,
            reply: Sender<Result<WindowsUiaEventSubscription, WindowsUiaWorkerError>>,
        },
        Drain {
            subscription: WindowsUiaEventSubscription,
            limit: usize,
            reply: Sender<Result<WindowsUiaEventDrain, WindowsUiaWorkerError>>,
        },
        Unsubscribe {
            subscription: WindowsUiaEventSubscription,
            reply: Sender<Result<(), WindowsUiaWorkerError>>,
        },
        Shutdown,
    }

    struct RegisteredSubscription {
        root: IUIAutomationElement,
        handler: IUIAutomationPropertyChangedEventHandler,
        buffer: Arc<Mutex<WindowsUiaEventBuffer>>,
        provider_incarnation_ref: ProviderIncarnationRef,
        target_incarnation_ref: TargetIncarnationRef,
    }

    struct EventWorkerState {
        automation: IUIAutomation,
        provider_incarnation_ref: ProviderIncarnationRef,
        subscriptions: HashMap<Uuid, RegisteredSubscription>,
    }

    #[windows::core::implement(IUIAutomationPropertyChangedEventHandler)]
    struct PropertyChangedHandler {
        buffer: Arc<Mutex<WindowsUiaEventBuffer>>,
    }

    impl IUIAutomationPropertyChangedEventHandler_Impl for PropertyChangedHandler_Impl {
        fn HandlePropertyChangedEvent(
            &self,
            _sender: Ref<'_, IUIAutomationElement>,
            property_id: UIA_PROPERTY_ID,
            _new_value: &VARIANT,
        ) -> windows::core::Result<()> {
            let mut buffer = self
                .buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = buffer.push(WindowsUiaEventDraft {
                kind: WindowsUiaEventKind::PropertyChanged {
                    property_id: property_id.0,
                },
                element_ref: None,
            });
            Ok(())
        }
    }

    pub struct WindowsUiaWorker {
        inner: crate::worker::WindowsUiaWorker,
        event_sender: Sender<EventWorkerCommand>,
        command_timeout: Duration,
    }

    impl fmt::Debug for WindowsUiaWorker {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("WindowsUiaWorker")
                .field("provider_incarnation_ref", self.provider_incarnation_ref())
                .field("command_timeout", &self.command_timeout)
                .finish_non_exhaustive()
        }
    }

    impl WindowsUiaWorker {
        pub fn capabilities() -> NativeProviderCapabilities {
            crate::worker::WindowsUiaWorker::capabilities()
        }

        pub fn spawn(config: WindowsUiaWorkerConfig) -> Result<Self, WindowsUiaWorkerError> {
            if config.command_timeout.is_zero() {
                return Err(WindowsUiaWorkerError::InvalidConfiguration);
            }

            let inner = crate::worker::WindowsUiaWorker::spawn(config.clone())?;
            let provider_incarnation_ref = inner.provider_incarnation_ref().clone();
            let (event_tx, event_rx) = mpsc::channel();
            let (startup_tx, startup_rx) = mpsc::sync_channel(1);
            thread::Builder::new()
                .name("localview-windows-uia-events-mta".into())
                .spawn(move || event_worker_main(event_rx, startup_tx, provider_incarnation_ref))
                .map_err(|error| WindowsUiaWorkerError::WorkerStartupFailed(error.to_string()))?;

            match startup_rx.recv_timeout(config.command_timeout) {
                Ok(Ok(())) => Ok(Self {
                    inner,
                    event_sender: event_tx,
                    command_timeout: config.command_timeout,
                }),
                Ok(Err(error)) => Err(error),
                Err(RecvTimeoutError::Timeout) => Err(WindowsUiaWorkerError::CommandTimeout),
                Err(RecvTimeoutError::Disconnected) => {
                    Err(WindowsUiaWorkerError::WorkerUnavailable)
                }
            }
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
        ) -> Result<WindowsUiaBoundDispatchContextReceipt, WindowsUiaWorkerError> {
            let requirements = request.requirements;
            self.inner
                .revalidate_dispatch_context(attachment, request)
                .map(|context| WindowsUiaBoundDispatchContextReceipt {
                    requirements,
                    context,
                })
        }

        pub fn dispatch_pattern(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaPatternDispatchRequest,
        ) -> Result<WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError> {
            self.inner.dispatch_pattern(attachment, request)
        }

        pub fn subscribe_events(
            &self,
            attachment: &WindowsUiaAttachment,
            options: WindowsUiaEventSubscriptionOptions,
        ) -> Result<WindowsUiaEventSubscription, WindowsUiaWorkerError> {
            if options.capacity == 0 {
                return Err(WindowsUiaWorkerError::InvalidConfiguration);
            }
            if attachment.provider_incarnation_ref() != self.provider_incarnation_ref() {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.event_sender
                .send(EventWorkerCommand::Subscribe {
                    attachment: attachment.clone(),
                    options,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_event_command(reply_rx, self.command_timeout)
        }

        pub fn drain_events(
            &self,
            subscription: &WindowsUiaEventSubscription,
            limit: usize,
        ) -> Result<WindowsUiaEventDrain, WindowsUiaWorkerError> {
            if subscription.provider_incarnation_ref != *self.provider_incarnation_ref() {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }
            if limit == 0 {
                return Err(WindowsUiaWorkerError::InvalidConfiguration);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.event_sender
                .send(EventWorkerCommand::Drain {
                    subscription: subscription.clone(),
                    limit,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_event_command(reply_rx, self.command_timeout)
        }

        pub fn unsubscribe_events(
            &self,
            subscription: WindowsUiaEventSubscription,
        ) -> Result<(), WindowsUiaWorkerError> {
            if subscription.provider_incarnation_ref != *self.provider_incarnation_ref() {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.event_sender
                .send(EventWorkerCommand::Unsubscribe {
                    subscription,
                    reply: reply_tx,
                })
                .map_err(|_| WindowsUiaWorkerError::WorkerUnavailable)?;
            recv_event_command(reply_rx, self.command_timeout)
        }
    }

    impl Drop for WindowsUiaWorker {
        fn drop(&mut self) {
            // Cleanup is deliberately asynchronous from the caller. The event MTA
            // owns all UIA registration objects and removes them before COM teardown.
            let _ = self.event_sender.send(EventWorkerCommand::Shutdown);
        }
    }

    fn recv_event_command<T>(
        receiver: Receiver<Result<T, WindowsUiaWorkerError>>,
        timeout: Duration,
    ) -> Result<T, WindowsUiaWorkerError> {
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(WindowsUiaWorkerError::CommandTimeout),
            Err(RecvTimeoutError::Disconnected) => Err(WindowsUiaWorkerError::WorkerUnavailable),
        }
    }

    fn event_worker_main(
        receiver: Receiver<EventWorkerCommand>,
        startup: mpsc::SyncSender<Result<(), WindowsUiaWorkerError>>,
        provider_incarnation_ref: ProviderIncarnationRef,
    ) {
        let initialized = unsafe {
            // SAFETY: this thread owns the event COM apartment for its full lifetime.
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()
        };
        if let Err(error) = initialized {
            let _ = startup.send(Err(WindowsUiaWorkerError::WorkerStartupFailed(
                error.to_string(),
            )));
            return;
        }

        let automation = unsafe {
            // SAFETY: COM is initialized above and this interface never leaves
            // the event worker thread.
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        };
        let automation = match automation {
            Ok(value) => value,
            Err(error) => {
                let _ = startup.send(Err(WindowsUiaWorkerError::WorkerStartupFailed(
                    error.to_string(),
                )));
                unsafe { CoUninitialize() };
                return;
            }
        };

        let mut state = EventWorkerState {
            automation,
            provider_incarnation_ref,
            subscriptions: HashMap::new(),
        };
        if startup.send(Ok(())).is_err() {
            unsafe { CoUninitialize() };
            return;
        }

        while let Ok(command) = receiver.recv() {
            match command {
                EventWorkerCommand::Subscribe {
                    attachment,
                    options,
                    reply,
                } => {
                    let _ = reply.send(state.subscribe(&attachment, options));
                }
                EventWorkerCommand::Drain {
                    subscription,
                    limit,
                    reply,
                } => {
                    let _ = reply.send(state.drain(&subscription, limit));
                }
                EventWorkerCommand::Unsubscribe {
                    subscription,
                    reply,
                } => {
                    let _ = reply.send(state.unsubscribe(&subscription));
                }
                EventWorkerCommand::Shutdown => break,
            }
        }

        state.unsubscribe_all_best_effort();
        drop(state);
        unsafe {
            // SAFETY: every apartment-owned interface is dropped above on this
            // exact thread before balancing CoInitializeEx.
            CoUninitialize();
        }
    }

    impl EventWorkerState {
        fn subscribe(
            &mut self,
            attachment: &WindowsUiaAttachment,
            options: WindowsUiaEventSubscriptionOptions,
        ) -> Result<WindowsUiaEventSubscription, WindowsUiaWorkerError> {
            if attachment.provider_incarnation_ref() != &self.provider_incarnation_ref {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }
            validate_target_fingerprint(attachment)?;

            let fingerprint = attachment.fingerprint();
            let root = unsafe {
                // SAFETY: the selected HWND was revalidated immediately above and
                // this UIA interface is confined to the event MTA.
                self.automation
                    .ElementFromHandle(hwnd_from_u64(fingerprint.native_window_handle))
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            let buffer = Arc::new(Mutex::new(
                WindowsUiaEventBuffer::new(
                    attachment.provider_incarnation_ref().clone(),
                    attachment.target_incarnation_ref().clone(),
                    options.capacity,
                )
                .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?,
            ));
            let handler: IUIAutomationPropertyChangedEventHandler = PropertyChangedHandler {
                buffer: Arc::clone(&buffer),
            }
            .into();

            unsafe {
                // SAFETY: root, automation and handler are all live on this MTA.
                // Subscribe only to scalar Name in this slice; array-valued UIA
                // properties are deliberately excluded from the callback ABI.
                self.automation.AddPropertyChangedEventHandlerNativeArray(
                    &root,
                    TreeScope_Subtree,
                    None,
                    &handler,
                    &[UIA_NamePropertyId],
                )
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

            let id = Uuid::new_v4();
            let reliability_profile = ProviderEventReliabilityProfile::windows_uia_v1();
            self.subscriptions.insert(
                id,
                RegisteredSubscription {
                    root,
                    handler,
                    buffer,
                    provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
                    target_incarnation_ref: attachment.target_incarnation_ref().clone(),
                },
            );

            Ok(WindowsUiaEventSubscription {
                id,
                provider_incarnation_ref: attachment.provider_incarnation_ref().clone(),
                target_incarnation_ref: attachment.target_incarnation_ref().clone(),
                sequence_baseline: 0,
                reliability_profile,
            })
        }

        fn drain(
            &mut self,
            subscription: &WindowsUiaEventSubscription,
            limit: usize,
        ) -> Result<WindowsUiaEventDrain, WindowsUiaWorkerError> {
            let registered = self.subscription(subscription)?;
            let mut buffer = registered
                .buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(buffer.drain(limit))
        }

        fn unsubscribe(
            &mut self,
            subscription: &WindowsUiaEventSubscription,
        ) -> Result<(), WindowsUiaWorkerError> {
            let registered = self.subscription(subscription)?;
            unsafe {
                // SAFETY: registration and removal occur on this same event MTA,
                // using the exact root and handler object used for registration.
                self.automation
                    .RemovePropertyChangedEventHandler(&registered.root, &registered.handler)
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;
            self.subscriptions.remove(&subscription.id);
            Ok(())
        }

        fn subscription(
            &self,
            subscription: &WindowsUiaEventSubscription,
        ) -> Result<&RegisteredSubscription, WindowsUiaWorkerError> {
            let registered = self.subscriptions.get(&subscription.id).ok_or_else(|| {
                WindowsUiaWorkerError::ProviderFailure(
                    "Windows UIA event subscription is no longer registered".into(),
                )
            })?;
            if registered.provider_incarnation_ref != subscription.provider_incarnation_ref
                || registered.target_incarnation_ref != subscription.target_incarnation_ref
            {
                return Err(WindowsUiaWorkerError::TargetReincarnated);
            }
            Ok(registered)
        }

        fn unsubscribe_all_best_effort(&mut self) {
            let ids = self.subscriptions.keys().copied().collect::<Vec<_>>();
            for id in ids {
                if let Some(registered) = self.subscriptions.remove(&id) {
                    let _ = unsafe {
                        // SAFETY: shutdown cleanup runs on the same MTA that
                        // registered each handler and retains both COM objects.
                        self.automation.RemovePropertyChangedEventHandler(
                            &registered.root,
                            &registered.handler,
                        )
                    };
                }
            }
        }
    }

    fn validate_target_fingerprint(
        attachment: &WindowsUiaAttachment,
    ) -> Result<(), WindowsUiaWorkerError> {
        let fingerprint = attachment.fingerprint();
        let hwnd = hwnd_from_u64(fingerprint.native_window_handle);
        let mut process_id = 0_u32;
        let thread_id = unsafe {
            // SAFETY: process_id is valid writable storage and HWND comes from the
            // already-attached explicit user-selected target fingerprint.
            GetWindowThreadProcessId(hwnd, Some(&mut process_id))
        };
        if thread_id == 0 || process_id != fingerprint.process_id {
            return Err(WindowsUiaWorkerError::TargetReincarnated);
        }
        if process_start_time_ticks(process_id)? != fingerprint.process_start_time_ticks {
            return Err(WindowsUiaWorkerError::TargetReincarnated);
        }
        Ok(())
    }

    fn hwnd_from_u64(value: u64) -> HWND {
        HWND(value as usize as *mut c_void)
    }

    fn process_start_time_ticks(process_id: u32) -> Result<u64, WindowsUiaWorkerError> {
        let process = unsafe {
            // SAFETY: this queries read-only process lifetime metadata.
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)
        }
        .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;

        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = unsafe {
            // SAFETY: all FILETIME buffers are valid for the duration of the call.
            GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user)
        };
        let _ = unsafe { CloseHandle(process) };
        result.map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;
        Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
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
        crate::worker::WindowsUiaWorker::capabilities()
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
    ) -> Result<std::sync::Arc<NativeSemanticSnapshotRevision>, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn revalidate_dispatch_context(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: WindowsUiaDispatchContextRequest,
    ) -> Result<WindowsUiaBoundDispatchContextReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
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
