#![forbid(unsafe_code)]

mod capture_settle;
mod fresh_snapshot;
mod perception;
mod perception_cycle;
mod perception_execution;
#[path = "runtime.rs"]
mod runtime;
mod visual_region;

use std::net::SocketAddr;

use anyhow::Result;
use axum::Router;

pub use runtime::{ControlState, EventEnvelope};
#[doc(hidden)]
pub use runtime::serve as legacy_serve;

pub fn router(state: ControlState) -> Router {
    runtime::router(state.clone())
        .merge(capture_settle::router(state.clone()))
        .merge(fresh_snapshot::router(state.clone()))
        .merge(perception::router(state.clone()))
        .merge(perception_execution::router(state.clone()))
        .merge(perception_cycle::router(state.clone()))
        .merge(visual_region::router(state))
}

pub async fn serve(addr: SocketAddr, state: ControlState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
