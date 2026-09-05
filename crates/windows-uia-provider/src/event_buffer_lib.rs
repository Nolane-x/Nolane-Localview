#![cfg_attr(not(windows), forbid(unsafe_code))]

mod action_capability;
mod dispatch_context;
mod event_buffer;
mod pattern_dispatch;
#[cfg(windows)]
mod subscription;
#[cfg(not(windows))]
#[path = "subscription_stub.rs"]
mod subscription;
#[path = "lib.rs"]
mod worker;

pub use action_capability::*;
pub use dispatch_context::*;
pub use event_buffer::*;
pub use pattern_dispatch::*;
pub use subscription::*;
pub use worker::{
    WindowsUiaAttachment, WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest,
    WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
