/// Provider-owned identity for one concrete lifetime of a semantic macOS application.
///
/// PID is intentionally only one component. `process_start_marker` must come from
/// the current process lifetime so PID reuse or relaunch cannot resurrect observer
/// or element authority from an earlier application incarnation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxApplicationIncarnation {
    application_identity: String,
    pid: i32,
    process_start_marker: u64,
}

impl AxApplicationIncarnation {
    pub fn new(
        application_identity: impl Into<String>,
        pid: i32,
        process_start_marker: u64,
    ) -> Self {
        Self {
            application_identity: application_identity.into(),
            pid,
            process_start_marker,
        }
    }

    pub fn application_identity(&self) -> &str {
        &self.application_identity
    }

    pub const fn pid(&self) -> i32 {
        self.pid
    }

    pub const fn process_start_marker(&self) -> u64 {
        self.process_start_marker
    }

    pub fn same_application_lineage(&self, other: &Self) -> bool {
        self.application_identity == other.application_identity
    }
}

/// Conversion boundary used by `AxElementIdentity::new`.
///
/// Shipping callers provide an explicit strong `AxApplicationIncarnation`.
/// The only PID-only conversion is compiled behind the validation-harness
/// feature so retained pre-M06 real-provider fixtures can be migrated without
/// exposing PID-only authority in the default production build.
pub trait IntoAxApplicationIncarnation {
    fn into_ax_application_incarnation(self) -> AxApplicationIncarnation;
}

impl IntoAxApplicationIncarnation for AxApplicationIncarnation {
    fn into_ax_application_incarnation(self) -> AxApplicationIncarnation {
        self
    }
}

#[cfg(feature = "validation-pid-identity")]
impl IntoAxApplicationIncarnation for i32 {
    fn into_ax_application_incarnation(self) -> AxApplicationIncarnation {
        AxApplicationIncarnation::new(
            format!("validation-harness-pid:{self}"),
            self,
            self as u64,
        )
    }
}
