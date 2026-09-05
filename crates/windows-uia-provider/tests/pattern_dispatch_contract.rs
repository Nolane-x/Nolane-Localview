use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaPatternDispatchReceipt, WindowsUiaPatternDispatchRequest,
    WindowsUiaWorker, WindowsUiaWorkerError,
};

fn assert_typed_dispatch_surface(
    worker: &WindowsUiaWorker,
    attachment: &WindowsUiaAttachment,
    request: WindowsUiaPatternDispatchRequest,
) -> Result<WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError> {
    worker.dispatch_pattern(attachment, request)
}

#[test]
fn real_pattern_dispatch_is_a_typed_worker_boundary() {
    let _ = assert_typed_dispatch_surface
        as fn(
            &WindowsUiaWorker,
            &WindowsUiaAttachment,
            WindowsUiaPatternDispatchRequest,
        ) -> Result<WindowsUiaPatternDispatchReceipt, WindowsUiaWorkerError>;
}
