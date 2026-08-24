#![forbid(unsafe_code)]

mod capture_settle;
mod fresh_snapshot;
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
        .merge(visual_region::router(state))
}

pub async fn serve(addr: SocketAddr, state: ControlState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
