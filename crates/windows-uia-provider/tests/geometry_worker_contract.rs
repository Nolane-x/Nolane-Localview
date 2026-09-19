use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaGeometryReceipt, WindowsUiaGeometryRequest, WindowsUiaWorker,
    WindowsUiaWorkerError,
};

#[test]
fn worker_exposes_exact_element_geometry_observation_surface() {
    let _observe: fn(
        &WindowsUiaWorker,
        &WindowsUiaAttachment,
        WindowsUiaGeometryRequest,
    ) -> Result<WindowsUiaGeometryReceipt, WindowsUiaWorkerError> = WindowsUiaWorker::observe_geometry;
}
