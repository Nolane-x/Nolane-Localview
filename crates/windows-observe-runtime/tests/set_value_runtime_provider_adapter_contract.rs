use localview_windows_observe_runtime::{
    WindowsUiaRuntimeDispatchExecutor, WindowsUiaSetValueExecutor,
};

fn assert_set_value_executor<T: WindowsUiaSetValueExecutor>() {}

#[test]
fn runtime_dispatch_executor_implements_set_value_execution_boundary() {
    assert_set_value_executor::<WindowsUiaRuntimeDispatchExecutor>();
}
