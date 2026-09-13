//! Provider-owned event reliability authority for macOS AX observers.
//!
//! M05 prevents a successful observer construction or a partially successful
//! notification registration set from being promoted into a false claim that
//! event delivery completely covers the requested semantic dimensions.
//! Unsupported or otherwise failed registrations remain explicit evidence and
//! require snapshot/reconciliation coverage for the missing dimensions.

const AX_ERROR_SUCCESS: i32 = 0;
const AX_ERROR_NOTIFICATION_UNSUPPORTED: i32 = -25207;

/// One notification LocalView asked the OS to register, bound to the semantic
/// dimension whose freshness would depend on that event channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxNotificationRequest {
    notification: String,
    semantic_dimension: String,
}

impl AxNotificationRequest {
    pub fn new(notification: impl Into<String>, semantic_dimension: impl Into<String>) -> Self {
        Self {
            notification: notification.into(),
            semantic_dimension: semantic_dimension.into(),
        }
    }

    pub fn notification(&self) -> &str {
        &self.notification
    }

    pub fn semantic_dimension(&self) -> &str {
        &self.semantic_dimension
    }
}

/// Exact provider interpretation of one `AXObserverAddNotification` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxNotificationRegistrationOutcome {
    Registered,
    Unsupported,
    Failed { ax_error: i32 },
}

/// Immutable evidence for one requested AX notification registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxNotificationRegistration {
    request: AxNotificationRequest,
    outcome: AxNotificationRegistrationOutcome,
}

impl AxNotificationRegistration {
    /// Classifies the raw AX registration result without caller-supplied
    /// completeness flags.
    pub fn from_ax_error(request: AxNotificationRequest, ax_error: i32) -> Self {
        let outcome = match ax_error {
            AX_ERROR_SUCCESS => AxNotificationRegistrationOutcome::Registered,
            AX_ERROR_NOTIFICATION_UNSUPPORTED => AxNotificationRegistrationOutcome::Unsupported,
            _ => AxNotificationRegistrationOutcome::Failed { ax_error },
        };

        Self { request, outcome }
    }

    pub fn request(&self) -> &AxNotificationRequest {
        &self.request
    }

    pub fn notification(&self) -> &str {
        self.request.notification()
    }

    pub fn semantic_dimension(&self) -> &str {
        self.request.semantic_dimension()
    }

    pub const fn outcome(&self) -> AxNotificationRegistrationOutcome {
        self.outcome
    }

    pub const fn is_registered(&self) -> bool {
        matches!(self.outcome, AxNotificationRegistrationOutcome::Registered)
    }
}

/// Event-channel assurance derived from exact notification registrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxEventAssurance {
    /// Every requested semantic dimension supplied to the profile has a
    /// successful notification registration.
    CompleteForRequestedDimensions,
    /// At least one requested dimension is unsupported/failed, or no requested
    /// dimensions were proven at all.
    Incomplete,
}

/// Provider-owned reduction of requested registrations into event assurance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxObserverReliabilityProfile {
    registrations: Vec<AxNotificationRegistration>,
    assurance: AxEventAssurance,
}

impl AxObserverReliabilityProfile {
    pub fn from_registrations(registrations: Vec<AxNotificationRegistration>) -> Self {
        let assurance = if !registrations.is_empty()
            && registrations.iter().all(AxNotificationRegistration::is_registered)
        {
            AxEventAssurance::CompleteForRequestedDimensions
        } else {
            AxEventAssurance::Incomplete
        };

        Self {
            registrations,
            assurance,
        }
    }

    pub const fn assurance(&self) -> AxEventAssurance {
        self.assurance
    }

    pub fn registrations(&self) -> &[AxNotificationRegistration] {
        &self.registrations
    }

    pub fn registered_notifications(&self) -> Vec<&str> {
        self.registrations
            .iter()
            .filter(|registration| registration.is_registered())
            .map(AxNotificationRegistration::notification)
            .collect()
    }

    pub fn unsupported_notifications(&self) -> Vec<&str> {
        self.registrations
            .iter()
            .filter(|registration| {
                registration.outcome() == AxNotificationRegistrationOutcome::Unsupported
            })
            .map(AxNotificationRegistration::notification)
            .collect()
    }

    pub fn incomplete_semantic_dimensions(&self) -> Vec<&str> {
        let mut dimensions: Vec<_> = self
            .registrations
            .iter()
            .filter(|registration| !registration.is_registered())
            .map(AxNotificationRegistration::semantic_dimension)
            .collect();
        dimensions.sort_unstable();
        dimensions.dedup();
        dimensions
    }

    pub const fn requires_snapshot_reconciliation(&self) -> bool {
        matches!(self.assurance, AxEventAssurance::Incomplete)
    }
}
