#![cfg(windows)]

use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaVerifiedInputRequest, WindowsUiaWorker,
    WindowsUiaWorkerError, WindowsVerifiedInputBoundaryReceipt,
};

#[test]
fn verified_input_is_owned_by_the_exact_uia_worker_boundary() {
    let _dispatch: fn(
        &WindowsUiaWorker,
        &WindowsUiaAttachment,
        WindowsUiaVerifiedInputRequest,
    ) -> Result<WindowsVerifiedInputBoundaryReceipt, WindowsUiaWorkerError> =
        WindowsUiaWorker::dispatch_verified_input;
}
