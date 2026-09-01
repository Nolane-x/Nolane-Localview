#![cfg_attr(not(windows), forbid(unsafe_code))]

mod event_buffer;
mod subscription;
#[path = "lib.rs"]
mod worker;

pub use event_buffer::*;
pub use subscription::*;
pub use worker::{
    WindowsUiaAttachment, WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
