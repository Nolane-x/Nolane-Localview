use localview_windows_observe_runtime::{
    WindowsUiaDispatchExecutionCoordinatorError, WindowsUiaDispatchExecutionResult,
    WindowsUiaDispatchExecutor, WindowsUiaProviderExecutionReceipt,
    WindowsUiaProviderExecutionRequest,
};

fn assert_executor_trait<T: WindowsUiaDispatchExecutor>() {}

#[test]
fn dispatch_executor_coordinator_api_is_explicit_and_typed() {
    let _ = std::any::type_name::<WindowsUiaProviderExecutionRequest>();
    let _ = std::any::type_name::<WindowsUiaProviderExecutionReceipt>();
    let _ = std::any::type_name::<WindowsUiaDispatchExecutionResult>();
    let _ = std::any::type_name::<WindowsUiaDispatchExecutionCoordinatorError>();
    let _ = assert_executor_trait::<NeverExecutor>;
}

struct NeverExecutor;
