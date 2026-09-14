#[path = "lib.rs"]
mod permission;
mod application_incarnation;
mod element_binding;
mod observer_application;
mod observer_continuity;
mod observer_reliability;
mod sensitive_text;
mod visual_permission;

pub use application_incarnation::*;
pub use element_binding::*;
pub use observer_application::*;
pub use observer_continuity::*;
pub use observer_reliability::*;
pub use permission::*;
pub use sensitive_text::*;
pub use visual_permission::*;

/// Shipping M07 continuity authority must not expose a callable raw AX callback
/// bridge. If these methods become available without the validation-only Cargo
/// feature, an ordinary caller could synthesize a callback and forge serviced
/// run-loop evidence without macOS delivering an event.
///
/// ```compile_fail
/// use localview_macos_ax_provider::AxRunLoopCallbackTracker;
///
/// let _ = AxRunLoopCallbackTracker::callback_function;
/// let _ = AxRunLoopCallbackTracker::callback_refcon;
/// ```
pub const M07_SHIPPING_CALLBACK_INJECTION_SURFACE_MUST_BE_ABSENT: () = ();
