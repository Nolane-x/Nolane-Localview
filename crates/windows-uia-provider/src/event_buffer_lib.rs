#![cfg_attr(not(windows), forbid(unsafe_code))]

mod event_buffer;
#[path = "lib.rs"]
mod worker;

pub use event_buffer::*;
pub use worker::*;
