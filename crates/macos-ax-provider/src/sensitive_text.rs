pub const AX_SECURE_TEXT_FIELD_SUBROLE: &str = "AXSecureTextField";
pub const PROTECTED_SECURE_TEXT_WIRE_VALUE: &str = "protected_secure_text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSensitiveTextProtection {
    Ordinary,
    ProtectedSecureText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxSensitiveTextTaint {
    Secret,
    Credential,
}

const NO_TAINTS: [AxSensitiveTextTaint; 0] = [];
const SECURE_TEXT_TAINTS: [AxSensitiveTextTaint; 2] = [
    AxSensitiveTextTaint::Secret,
    AxSensitiveTextTaint::Credential,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSensitiveTextMetadata<'a> {
    role: &'a str,
    subrole: Option<&'a str>,
}

impl<'a> AxSensitiveTextMetadata<'a> {
    pub const fn new(role: &'a str, subrole: Option<&'a str>) -> Self {
        Self { role, subrole }
    }

    pub const fn role(self) -> &'a str { self.role }
    pub const fn subrole(self) -> Option<&'a str> { self.subrole }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxSensitiveTextDecision {
    protection: AxSensitiveTextProtection,
}

impl AxSensitiveTextDecision {
    pub const fn protection(self) -> AxSensitiveTextProtection { self.protection }
    pub const fn value_read_permitted(self) -> bool {
        !matches!(self.protection, AxSensitiveTextProtection::ProtectedSecureText)
    }
    pub const fn semantic_text_protection(self) -> Option<&'static str> {
        match self.protection {
            AxSensitiveTextProtection::Ordinary => None,
            AxSensitiveTextProtection::ProtectedSecureText => Some(PROTECTED_SECURE_TEXT_WIRE_VALUE),
        }
    }
    pub const fn taints(self) -> &'static [AxSensitiveTextTaint] {
        match self.protection {
            AxSensitiveTextProtection::Ordinary => &NO_TAINTS,
            AxSensitiveTextProtection::ProtectedSecureText => &SECURE_TEXT_TAINTS,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AxSensitiveTextPolicy;

impl AxSensitiveTextPolicy {
    pub fn classify(metadata: AxSensitiveTextMetadata<'_>) -> AxSensitiveTextDecision {
        let protection = match metadata.subrole {
            Some(AX_SECURE_TEXT_FIELD_SUBROLE) => AxSensitiveTextProtection::ProtectedSecureText,
            _ => AxSensitiveTextProtection::Ordinary,
        };
        AxSensitiveTextDecision { protection }
    }
}
