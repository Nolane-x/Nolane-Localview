#![forbid(unsafe_code)]

#[cfg(target_os = "linux")]
mod binding;
#[cfg(target_os = "linux")]
mod error;
#[cfg(target_os = "linux")]
mod provider;

#[cfg(target_os = "linux")]
pub use binding::{
    AtspiActionEligibilityPermit, AtspiBindingLifecycle, AtspiElementBinding, AtspiEndpoint,
};
#[cfg(target_os = "linux")]
pub use error::{AtspiActionEligibilityError, AtspiProviderConnectionError};
#[cfg(target_os = "linux")]
pub use provider::LinuxAtspiProvider;
