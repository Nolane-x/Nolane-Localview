from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one anchor, found {count}")
    return text.replace(old, new, 1)


lib_path = Path("crates/windows-uia-provider/src/lib.rs")
text = lib_path.read_text()

text = replace_once(
    text,
    '''    #[error("Windows UI Automation element lease request is invalid")]
    InvalidElementLeaseRequest,
    #[error("Windows UI Automation dispatch context request is invalid")]''',
    '''    #[error("Windows UI Automation element lease request is invalid")]
    InvalidElementLeaseRequest,
    #[error("Windows UI Automation geometry request is invalid")]
    InvalidGeometryRequest,
    #[error("Windows UI Automation dispatch context request is invalid")]''',
    "worker error",
)

text = replace_once(
    text,
    '''            WindowsAndMessaging::{
                GW_ENABLEDPOPUP, GetForegroundWindow, GetWindow, GetWindowThreadProcessId,''',
    '''            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                GW_ENABLEDPOPUP, GetForegroundWindow, GetWindow, GetWindowThreadProcessId,''',
    "GetDpiForWindow import",
)

text = replace_once(
    text,
    '''        WindowsInputDispatchBlocker, WindowsUiaActionCapabilities, WindowsUiaBooleanCapabilityFact,
        WindowsUiaDispatchContextObservation, WindowsUiaDispatchContextReceipt,''',
    '''        WindowsInputDispatchBlocker, WindowsUiaActionCapabilities, WindowsUiaBooleanCapabilityFact,
        WindowsUiaCoordinateSpace, WindowsUiaDispatchContextObservation,
        WindowsUiaDispatchContextReceipt, WindowsUiaGeometryReceipt, WindowsUiaGeometryRequest,
        WindowsUiaPhysicalRect,''',
    "geometry imports",
)

text = replace_once(
    text,
    '''        BindElementLease {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaElementLeaseRequest,
            reply: Sender<Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError>>,
        },
        RevalidateDispatchContext {''',
    '''        BindElementLease {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaElementLeaseRequest,
            reply: Sender<Result<WindowsUiaElementLeaseReceipt, WindowsUiaWorkerError>>,
        },
        ObserveGeometry {
            attachment: WindowsUiaAttachment,
            request: WindowsUiaGeometryRequest,
            reply: Sender<Result<WindowsUiaGeometryReceipt, WindowsUiaWorkerError>>,
        },
        RevalidateDispatchContext {''',
    "worker command",
)

text = replace_once(
    text,
    '''        pub fn revalidate_dispatch_context(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaDispatchContextRequest,''',
    '''        pub fn observe_geometry(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaGeometryRequest,
        ) -> Result<WindowsUiaGeometryReceipt, WindowsUiaWorkerError> {
            let element_ref = request.element_ref();
            if attachment.provider_incarnation_ref != self.provider_incarnation_ref
                || element_ref.provider_incarnation_ref != self.provider_incarnation_ref
                || element_ref.target_incarnation_ref != attachment.target_incarnation_ref
                || element_ref.acquisition_cut_ref != request.snapshot_cut_ref()
            {
                return Err(WindowsUiaWorkerError::InvalidGeometryRequest);
            }

            let (reply_tx, reply_rx) = mpsc::channel();
            self.ensure_healthy()?;
            self.sender
                .send(WorkerCommand::ObserveGeometry {
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
            request: WindowsUiaDispatchContextRequest,''',
    "public observe_geometry",
)

text = replace_once(
    text,
    '''                WorkerCommand::RevalidateDispatchContext {
                    attachment,
                    request,
                    reply,
                } => {''',
    '''                WorkerCommand::ObserveGeometry {
                    attachment,
                    request,
                    reply,
                } => {
                    let _ = reply.send(state.observe_geometry(&attachment, request));
                }
                WorkerCommand::RevalidateDispatchContext {
                    attachment,
                    request,
                    reply,
                } => {''',
    "worker main dispatch",
)

text = replace_once(
    text,
    '''        fn query_virtualized_item(
            &mut self,
            attachment: &WindowsUiaAttachment,''',
    '''        fn observe_geometry(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaGeometryRequest,
        ) -> Result<WindowsUiaGeometryReceipt, WindowsUiaWorkerError> {
            let retained = self.exact_retained_element(
                attachment,
                request.snapshot_cut_ref(),
                request.element_ref(),
            )?;
            let rect = unsafe {
                // SAFETY: the exact retained UIA element remains owned by this worker MTA.
                retained.element.CurrentBoundingRectangle()
            }
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;
            let bounding_rect = WindowsUiaPhysicalRect::new(
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
            )
            .map_err(|error| WindowsUiaWorkerError::ProviderFailure(error.to_string()))?;
            let target_window_dpi = unsafe {
                // SAFETY: exact_retained_element revalidated this attachment HWND immediately above.
                GetDpiForWindow(hwnd_from_u64(attachment.selection.native_window_handle))
            };
            if target_window_dpi == 0 {
                return Err(WindowsUiaWorkerError::ProviderFailure(
                    "GetDpiForWindow returned zero for the exact attached HWND".into(),
                ));
            }

            Ok(WindowsUiaGeometryReceipt {
                snapshot_cut_ref: request.snapshot_cut_ref().to_owned(),
                provider_incarnation_ref: self.provider_incarnation_ref.clone(),
                target_incarnation_ref: attachment.target_incarnation_ref.clone(),
                element_ref: retained.element_ref.clone(),
                coordinate_space: WindowsUiaCoordinateSpace::PhysicalScreenPixels,
                bounding_rect,
                target_window_dpi,
            })
        }

        fn query_virtualized_item(
            &mut self,
            attachment: &WindowsUiaAttachment,''',
    "worker state observe_geometry",
)

text = replace_once(
    text,
    '''    pub fn revalidate_dispatch_context(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaDispatchContextRequest,''',
    '''    pub fn observe_geometry(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaGeometryRequest,
    ) -> Result<crate::WindowsUiaGeometryReceipt, WindowsUiaWorkerError> {
        Err(WindowsUiaWorkerError::UnsupportedPlatform)
    }

    pub fn revalidate_dispatch_context(
        &self,
        _attachment: &WindowsUiaAttachment,
        _request: crate::WindowsUiaDispatchContextRequest,''',
    "non-Windows observe_geometry",
)

lib_path.write_text(text)

windows_wrapper_path = Path("crates/windows-uia-provider/src/subscription.rs")
windows_wrapper = windows_wrapper_path.read_text()
windows_wrapper = replace_once(
    windows_wrapper,
    '''        pub fn revalidate_dispatch_context(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaDispatchContextRequest,''',
    '''        pub fn observe_geometry(
            &self,
            attachment: &WindowsUiaAttachment,
            request: crate::WindowsUiaGeometryRequest,
        ) -> Result<crate::WindowsUiaGeometryReceipt, WindowsUiaWorkerError> {
            self.inner.observe_geometry(attachment, request)
        }

        pub fn revalidate_dispatch_context(
            &self,
            attachment: &WindowsUiaAttachment,
            request: WindowsUiaDispatchContextRequest,''',
    "public geometry wrapper (subscription.rs)",
)
windows_wrapper_path.write_text(windows_wrapper)

stub_wrapper_path = Path("crates/windows-uia-provider/src/subscription_stub.rs")
stub_wrapper = stub_wrapper_path.read_text()
stub_wrapper = replace_once(
    stub_wrapper,
    '''    pub fn revalidate_dispatch_context(
        &self,
        attachment: &WindowsUiaAttachment,
        request: WindowsUiaDispatchContextRequest,''',
    '''    pub fn observe_geometry(
        &self,
        attachment: &WindowsUiaAttachment,
        request: crate::WindowsUiaGeometryRequest,
    ) -> Result<crate::WindowsUiaGeometryReceipt, WindowsUiaWorkerError> {
        self.inner.observe_geometry(attachment, request)
    }

    pub fn revalidate_dispatch_context(
        &self,
        attachment: &WindowsUiaAttachment,
        request: WindowsUiaDispatchContextRequest,''',
    "public geometry wrapper (subscription_stub.rs)",
)
stub_wrapper_path.write_text(stub_wrapper)

cargo_path = Path("crates/windows-uia-provider/Cargo.toml")
cargo = cargo_path.read_text()
cargo = replace_once(
    cargo,
    '''  "Win32_UI_Accessibility",
  "Win32_UI_Input_KeyboardAndMouse",''',
    '''  "Win32_UI_Accessibility",
  "Win32_UI_HiDpi",
  "Win32_UI_Input_KeyboardAndMouse",''',
    "HiDpi feature",
)
cargo_path.write_text(cargo)
