use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaVirtualizedItemQueryReceipt,
    WindowsUiaVirtualizedItemQueryRequest, WindowsUiaVirtualizedItemRealizeReceipt,
    WindowsUiaVirtualizedItemRealizeRequest, WindowsUiaWorker, WindowsUiaWorkerError,
};

// Compile-time contract only. Real provider behavior is covered separately on a
// hosted Windows runner after the MTA implementation exists. Keeping this test
// platform-neutral makes accidental removal of the worker capability visible to
// every workspace compiler gate.
#[allow(dead_code)]
fn query_virtualized_item_surface(
    worker: &WindowsUiaWorker,
    attachment: &WindowsUiaAttachment,
    request: WindowsUiaVirtualizedItemQueryRequest,
) -> Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> {
    worker.query_virtualized_item(attachment, request)
}

#[allow(dead_code)]
fn realize_virtualized_item_surface(
    worker: &WindowsUiaWorker,
    attachment: &WindowsUiaAttachment,
    request: WindowsUiaVirtualizedItemRealizeRequest,
) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> {
    worker.realize_virtualized_item(attachment, request)
}

#[test]
fn worker_surface_is_type_checked_on_every_platform() {
    let query: fn(
        &WindowsUiaWorker,
        &WindowsUiaAttachment,
        WindowsUiaVirtualizedItemQueryRequest,
    ) -> Result<WindowsUiaVirtualizedItemQueryReceipt, WindowsUiaWorkerError> =
        WindowsUiaWorker::query_virtualized_item;
    let realize: fn(
        &WindowsUiaWorker,
        &WindowsUiaAttachment,
        WindowsUiaVirtualizedItemRealizeRequest,
    ) -> Result<WindowsUiaVirtualizedItemRealizeReceipt, WindowsUiaWorkerError> =
        WindowsUiaWorker::realize_virtualized_item;

    let _ = (query, realize);
}
