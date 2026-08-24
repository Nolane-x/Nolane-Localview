#![forbid(unsafe_code)]

mod capture_settle;
#[path = "runtime.rs"]
mod runtime;

use std::net::SocketAddr;

use anyhow::Result;
use axum::Router;

pub use runtime::{ControlState, EventEnvelope};

pub fn router(state: ControlState) -> Router {
    runtime::router(state.clone()).merge(capture_settle::router(state))
}

pub async fn serve(addr: SocketAddr, state: ControlState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}
