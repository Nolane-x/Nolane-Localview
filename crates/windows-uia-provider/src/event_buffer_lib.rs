#![cfg_attr(not(windows), forbid(unsafe_code))]

mod event_buffer;
#[cfg(windows)]
mod subscription;
#[cfg(not(windows))]
#[path = "subscription_stub.rs"]
mod subscription;
#[path = "lib.rs"]
mod worker;

pub use event_buffer::*;
pub use subscription::*;
pub use worker::{
    WindowsUiaAttachment, WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
