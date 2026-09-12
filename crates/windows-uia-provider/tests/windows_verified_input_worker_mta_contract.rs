#![cfg(windows)]

use localview_windows_uia_provider::{
    WindowsUiaAttachment, WindowsUiaVerifiedInputReceipt, WindowsUiaVerifiedInputRequest,
    WindowsUiaWorker, WindowsUiaWorkerError,
};

#[test]
fn verified_input_is_owned_by_the_exact_uia_worker_boundary() {
    let _dispatch: fn(
        &WindowsUiaWorker,
        &WindowsUiaAttachment,
        WindowsUiaVerifiedInputRequest,
    ) -> Result<WindowsUiaVerifiedInputReceipt, WindowsUiaWorkerError> =
        WindowsUiaWorker::dispatch_verified_input;
}
