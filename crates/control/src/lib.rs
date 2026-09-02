#![forbid(unsafe_code)]

mod action_cancellation;
mod capture_settle;
mod chromium_runtime;
mod fresh_snapshot;
mod native_cancellation;
mod native_executor;
mod perception;
mod perception_cycle;
mod perception_execution;
mod resource_runtime;
#[path = "runtime.rs"]
mod runtime;
mod visual_diff;
mod visual_region;
mod visual_verify;
mod windows_observe;

use std::net::SocketAddr;

use anyhow::Result;
use axum::Router;

#[doc(hidden)]
pub use chromium_runtime::configure_chromium_executor_for_sessions;
#[doc(hidden)]
pub use native_executor::{
    wait_for_native_executor_result_with_timeout, NativeExecutorWaitError,
};
pub use localview_resource_governor::RuntimeResourceGovernor;
pub use resource_runtime::runtime_resource_governor_for_sessions;
#[doc(hidden)]
pub use runtime::serve as legacy_serve;
pub use runtime::{ControlState, EventEnvelope};
pub use windows_observe::{
    configure_windows_observe_runtime_for_sessions, windows_observe_runtime_for_sessions,
};

pub fn router(state: ControlState) -> Router {
    runtime::router(state.clone())
        .merge(action_cancellation::router(state.clone()))
        .merge(capture_settle::router(state.clone()))
        .merge(fresh_snapshot::router(state.clone()))
        .merge(native_cancellation::router(state.clone()))
        .merge(native_executor::router(state.clone()))
        .merge(perception::router(state.clone()))
        .merge(perception_execution::router(state.clone()))
        .merge(perception_cycle::router(state.clone()))
        .merge(resource_runtime::router(state.clone()))
        .merge(visual_diff::router(state.clone()))
        .merge(visual_verify::router(state.clone()))
        .merge(windows_observe::router(state.clone()))
        .merge(visual_region::router(state))
}

pub async fn serve(addr: SocketAddr, state: ControlState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
