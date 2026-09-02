use localview_windows_observe_runtime::{
    arm_uia_dispatch_execution, WindowsUiaDispatchExecutionArmError,
    WindowsUiaDispatchExecutionPermit,
};

#[test]
fn execution_arm_api_is_explicit_and_typed() {
    let _ = std::any::type_name::<WindowsUiaDispatchExecutionPermit>();
    let _ = std::any::type_name::<WindowsUiaDispatchExecutionArmError>();
    let _ = arm_uia_dispatch_execution;
}
