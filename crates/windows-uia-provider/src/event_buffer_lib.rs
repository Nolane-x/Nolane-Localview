#![cfg_attr(not(windows), forbid(unsafe_code))]

mod action_capability;
mod custom_semantic_coverage;
#[cfg(test)]
mod custom_semantic_coverage_contract;
mod dispatch_context;
mod event_buffer;
mod geometry;
mod pattern_dispatch;
mod set_value_dispatch;
#[cfg(windows)]
mod subscription;
#[cfg(not(windows))]
#[path = "subscription_stub.rs"]
mod subscription;
mod verified_input;
#[cfg(windows)]
mod verified_input_windows;
mod virtualized_item;
mod w13_semantic_text_protection;
#[cfg(test)]
mod w13_semantic_text_protection_contract;
#[path = "lib.rs"]
mod worker;
#[cfg(windows)]
mod worker_health;

pub use action_capability::*;
pub use custom_semantic_coverage::{
    WINDOWS_UIA_ACCESSIBILITY_OPAQUE_CUSTOM_CONTROL_DEBT,
    WINDOWS_UIA_ACCESSIBILITY_PARTIAL_CUSTOM_CONTROL_DEBT,
    WINDOWS_UIA_CUSTOM_SEMANTIC_COVERAGE_ATTRIBUTE, WindowsUiaCustomSemanticCoverage,
};
pub use dispatch_context::*;
pub use event_buffer::*;
pub use geometry::*;
pub use pattern_dispatch::*;
pub use set_value_dispatch::*;
pub use subscription::*;
pub use verified_input::*;
#[cfg(windows)]
pub(crate) use verified_input_windows::windows_insert_verified_key_events;
#[cfg(windows)]
pub use verified_input_windows::{
    observe_windows_verified_input_context, snapshot_windows_keyboard_state,
};
pub use virtualized_item::*;
pub use worker::{
    WindowsUiaAttachment, WindowsUiaElementLeaseReceipt, WindowsUiaElementLeaseRequest,
    WindowsUiaSnapshotRequest, WindowsUiaWorkerConfig, WindowsUiaWorkerError,
};
