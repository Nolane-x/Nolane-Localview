pub const AX_TEXT_FIELD_ROLE: &str = "AXTextField";
pub const AX_SECURE_TEXT_FIELD_SUBROLE: &str = "AXSecureTextField";
pub const PROTECTED_SECURE_TEXT_WIRE_VALUE: &str = "protected_secure_text";
pub const OPAQUE_UNKNOWN_SENSITIVITY_WIRE_VALUE: &str = "opaque_unknown_sensitivity";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSensitiveTextProtection {
    Ordinary,
    ProtectedSecureText,
    OpaqueUnknownSensitivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSensitiveTextTaint {
    Secret,
    Credential,
}

const NO_TAINTS: [AxSensitiveTextTaint; 0] = [];
const SENSITIVE_TEXT_TAINTS: [AxSensitiveTextTaint; 2] = [
    AxSensitiveTextTaint::Secret,
    AxSensitiveTextTaint::Credential,
];

/// Result of observing the AXSubrole attribute.
///
/// `Absent` means the provider successfully established that no subrole is
/// present. `Unknown` means the subrole could not be observed conclusively;
/// callers must not launder that failure into `Absent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSensitiveTextSubroleObservation<'a> {
    Present(&'a str),
    Absent,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSensitiveTextMetadata<'a> {
    role: &'a str,
    subrole: AxSensitiveTextSubroleObservation<'a>,
}

impl<'a> AxSensitiveTextMetadata<'a> {
    /// Constructs metadata from a conclusive AXSubrole observation.
    ///
    /// `None` means the provider successfully observed that the element has no
    /// subrole. If AXSubrole lookup fails or is otherwise inconclusive, use
    /// [`Self::unknown_subrole`] instead.
    pub const fn new(role: &'a str, subrole: Option<&'a str>) -> Self {
        Self {
            role,
            subrole: match subrole {
                Some(subrole) => AxSensitiveTextSubroleObservation::Present(subrole),
                None => AxSensitiveTextSubroleObservation::Absent,
            },
        }
    }

    /// Constructs metadata for an inconclusive AXSubrole observation.
    ///
    /// Text fields with unknown subrole fail closed before AXValue exposure.
    pub const fn unknown_subrole(role: &'a str) -> Self {
        Self {
            role,
            subrole: AxSensitiveTextSubroleObservation::Unknown,
        }
    }

    pub const fn role(self) -> &'a str {
        self.role
    }

    /// Compatibility projection for conclusively present subroles.
    ///
    /// Both `Absent` and `Unknown` project to `None`; policy decisions must use
    /// [`Self::subrole_observation`] so uncertainty is never lost.
    pub const fn subrole(self) -> Option<&'a str> {
        match self.subrole {
            AxSensitiveTextSubroleObservation::Present(subrole) => Some(subrole),
            AxSensitiveTextSubroleObservation::Absent | AxSensitiveTextSubroleObservation::Unknown => None,
        }
    }

    pub const fn subrole_observation(self) -> AxSensitiveTextSubroleObservation<'a> {
        self.subrole
    }
}

/// Provider-owned sensitivity decision. Its protection field is private so a
/// caller cannot forge a permissive decision for an uncertain text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSensitiveTextDecision {
    protection: AxSensitiveTextProtection,
}

impl AxSensitiveTextDecision {
    pub const fn protection(self) -> AxSensitiveTextProtection {
        self.protection
    }

    pub const fn value_read_permitted(self) -> bool {
        matches!(self.protection, AxSensitiveTextProtection::Ordinary)
    }

    pub const fn semantic_text_protection(self) -> Option<&'static str> {
        match self.protection {
            AxSensitiveTextProtection::Ordinary => None,
            AxSensitiveTextProtection::ProtectedSecureText => Some(PROTECTED_SECURE_TEXT_WIRE_VALUE),
            AxSensitiveTextProtection::OpaqueUnknownSensitivity => {
                Some(OPAQUE_UNKNOWN_SENSITIVITY_WIRE_VALUE)
            }
        }
    }

    pub const fn taints(self) -> &'static [AxSensitiveTextTaint] {
        match self.protection {
            AxSensitiveTextProtection::Ordinary => &NO_TAINTS,
            AxSensitiveTextProtection::ProtectedSecureText
            | AxSensitiveTextProtection::OpaqueUnknownSensitivity => &SENSITIVE_TEXT_TAINTS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AxSensitiveTextPolicy;

impl AxSensitiveTextPolicy {
    pub fn classify(metadata: AxSensitiveTextMetadata<'_>) -> AxSensitiveTextDecision {
        let protection = match metadata.subrole_observation() {
            AxSensitiveTextSubroleObservation::Present(AX_SECURE_TEXT_FIELD_SUBROLE) => {
                AxSensitiveTextProtection::ProtectedSecureText
            }
            AxSensitiveTextSubroleObservation::Unknown if metadata.role() == AX_TEXT_FIELD_ROLE => {
                AxSensitiveTextProtection::OpaqueUnknownSensitivity
            }
            AxSensitiveTextSubroleObservation::Present(_)
            | AxSensitiveTextSubroleObservation::Absent
            | AxSensitiveTextSubroleObservation::Unknown => AxSensitiveTextProtection::Ordinary,
        };
        AxSensitiveTextDecision { protection }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_text_field_subrole_is_opaque_and_tainted() {
        let decision = AxSensitiveTextPolicy::classify(AxSensitiveTextMetadata::unknown_subrole(
            AX_TEXT_FIELD_ROLE,
        ));

        assert_eq!(
            decision.protection(),
            AxSensitiveTextProtection::OpaqueUnknownSensitivity
        );
        assert!(!decision.value_read_permitted());
        assert_eq!(
            decision.semantic_text_protection(),
            Some(OPAQUE_UNKNOWN_SENSITIVITY_WIRE_VALUE)
        );
        assert_eq!(decision.taints(), &SENSITIVE_TEXT_TAINTS);
    }
}
