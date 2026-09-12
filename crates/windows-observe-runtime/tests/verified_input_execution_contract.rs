use std::fs;

use localview_windows_observe_runtime::{
    WindowsUiaVerifiedInputExecutionCoordinatorError, WindowsUiaVerifiedInputExecutor,
    WindowsUiaVerifiedInputProviderReceipt,
};

#[test]
fn runtime_owns_verified_input_request_minting_and_exact_binding() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let execution_arm = fs::read_to_string(format!("{manifest}/src/execution_arm.rs"))
        .expect("read execution_arm.rs");
    let runtime_manager = fs::read_to_string(format!("{manifest}/src/runtime_manager.rs"))
        .expect("read runtime_manager.rs");

    assert!(
        execution_arm.contains("WindowsUiaVerifiedInputRequest::from_execution_permit"),
        "runtime coordinator must mint the provider request from the hidden one-shot permit"
    );
    assert!(
        execution_arm.contains("verified_input_receipt_matches_request"),
        "runtime must compare exact provider receipt binding before journal linearization"
    );
    assert!(
        execution_arm.contains("record_dispatch_linearized"),
        "verified input must reuse the canonical consequential journal writer"
    );
    assert!(
        runtime_manager.contains(
            "impl crate::WindowsUiaVerifiedInputExecutor for WindowsUiaRuntimeDispatchExecutor"
        ),
        "production runtime executor must route verified input through the existing attached worker"
    );

    fn assert_error(error: WindowsUiaVerifiedInputExecutionCoordinatorError) {
        let _ = error;
    }
    fn assert_receipt(receipt: WindowsUiaVerifiedInputProviderReceipt) {
        let _ = receipt;
    }
    fn assert_executor<T: WindowsUiaVerifiedInputExecutor>() {}
    let _ = assert_error as fn(WindowsUiaVerifiedInputExecutionCoordinatorError);
    let _ = assert_receipt as fn(WindowsUiaVerifiedInputProviderReceipt);
    let _ = assert_executor::<localview_windows_observe_runtime::WindowsUiaRuntimeDispatchExecutor>;
}
