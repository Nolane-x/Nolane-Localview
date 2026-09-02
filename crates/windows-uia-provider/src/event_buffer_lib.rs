#![cfg_attr(not(windows), forbid(unsafe_code))]

mod action_capability;
mod event_buffer;
#[cfg(windows)]
mod subscription;
#[cfg(not(windows))]
#[path = "subscription_stub.rs"]
mod subscription;
#[path = "lib.rs"]
mod worker;

pub use action_capability::*;
pub use event_buffer::*;
pub use subscription::*;
pub use worker::{
    WindowsUiaAttachment, WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest,
    WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
