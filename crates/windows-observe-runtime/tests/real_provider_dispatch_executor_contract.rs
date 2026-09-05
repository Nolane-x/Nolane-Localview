use localview_protocol::SessionId;
use localview_windows_observe_runtime::{
    WindowsObserveRuntimeManager, WindowsUiaDispatchExecutor, WindowsUiaRuntimeDispatchExecutor,
};
use localview_windows_uia_provider::WindowsUiaWorker;

fn assert_dispatch_executor<T: WindowsUiaDispatchExecutor>() {}

async fn resolve_attached_executor(
    runtime: &WindowsObserveRuntimeManager<WindowsUiaWorker>,
    session_id: SessionId,
) {
    let _: WindowsUiaRuntimeDispatchExecutor = runtime
        .uia_dispatch_executor(session_id)
        .await
        .expect("attached Windows UIA session must expose only its exact dispatch executor");
}

#[test]
fn real_windows_uia_executor_is_the_runtime_one_shot_provider_boundary() {
    assert_dispatch_executor::<WindowsUiaRuntimeDispatchExecutor>();
    let _ = resolve_attached_executor;
}
