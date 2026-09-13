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
