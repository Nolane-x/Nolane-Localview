use std::collections::BTreeMap;

use localview_native_provider::NativeSemanticNodeObservation;

pub const WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1: &str = "windows-uia-action-capabilities-v1";
const PROFILE_ATTRIBUTE_KEY: &str = "windows_uia.action_capability_profile";
const IS_PASSWORD_ATTRIBUTE_KEY: &str = "windows_uia.is_password";
const VALUE_IS_READ_ONLY_ATTRIBUTE_KEY: &str = "windows_uia.value.is_read_only";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WindowsUiaPattern {
    Invoke,
    SelectionItem,
    Value,
    Toggle,
    ExpandCollapse,
    ScrollItem,
    VirtualizedItem,
}

impl WindowsUiaPattern {
    pub const ALL: [Self; 7] = [
        Self::Invoke,
        Self::SelectionItem,
        Self::Value,
        Self::Toggle,
        Self::ExpandCollapse,
        Self::ScrollItem,
        Self::VirtualizedItem,
    ];

    pub const fn attribute_key(self) -> &'static str {
        match self {
            Self::Invoke => "windows_uia.pattern.invoke",
            Self::SelectionItem => "windows_uia.pattern.selection_item",
            Self::Value => "windows_uia.pattern.value",
            Self::Toggle => "windows_uia.pattern.toggle",
            Self::ExpandCollapse => "windows_uia.pattern.expand_collapse",
            Self::ScrollItem => "windows_uia.pattern.scroll_item",
            Self::VirtualizedItem => "windows_uia.pattern.virtualized_item",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsUiaPatternSupport {
    Supported,
    Unsupported,
    Unknown,
}

impl WindowsUiaPatternSupport {
    pub const fn as_wire_value(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }

    fn from_wire_value(value: &str) -> Self {
        match value {
            "supported" => Self::Supported,
            "unsupported" => Self::Unsupported,
            "unknown" => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowsUiaBooleanCapabilityFact {
    True,
    False,
    #[default]
    Unknown,
}

impl WindowsUiaBooleanCapabilityFact {
    pub const fn from_bool(value: bool) -> Self {
        if value { Self::True } else { Self::False }
    }

    pub const fn as_wire_value(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }

    fn from_wire_value(value: &str) -> Self {
        match value {
            "true" => Self::True,
            "false" => Self::False,
            "unknown" => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowsUiaActionCapabilities {
    patterns: BTreeMap<WindowsUiaPattern, WindowsUiaPatternSupport>,
}

impl WindowsUiaActionCapabilities {
    pub const fn profile_attribute_key() -> &'static str {
        PROFILE_ATTRIBUTE_KEY
    }

    pub fn from_node(node: &NativeSemanticNodeObservation) -> Self {
        if !has_declared_capability_profile(node) {
            return Self::default();
        }

        let patterns = WindowsUiaPattern::ALL
            .into_iter()
            .map(|pattern| {
                let support = node
                    .attributes
                    .get(pattern.attribute_key())
                    .map(|value| WindowsUiaPatternSupport::from_wire_value(value))
                    .unwrap_or(WindowsUiaPatternSupport::Unknown);
                (pattern, support)
            })
            .collect();
        Self { patterns }
    }

    pub fn support_for(&self, pattern: WindowsUiaPattern) -> WindowsUiaPatternSupport {
        self.patterns
            .get(&pattern)
            .copied()
            .unwrap_or(WindowsUiaPatternSupport::Unknown)
    }

    pub fn record(&mut self, pattern: WindowsUiaPattern, support: WindowsUiaPatternSupport) {
        self.patterns.insert(pattern, support);
    }

    pub fn write_attributes(&self, attributes: &mut BTreeMap<String, String>) {
        attributes.insert(
            PROFILE_ATTRIBUTE_KEY.into(),
            WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1.into(),
        );
        for pattern in WindowsUiaPattern::ALL {
            attributes.insert(
                pattern.attribute_key().into(),
                self.support_for(pattern).as_wire_value().into(),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsUiaValueCapabilityFacts {
    value_support: WindowsUiaPatternSupport,
    is_password: WindowsUiaBooleanCapabilityFact,
    is_read_only: WindowsUiaBooleanCapabilityFact,
}

impl Default for WindowsUiaValueCapabilityFacts {
    fn default() -> Self {
        Self::new(
            WindowsUiaPatternSupport::Unknown,
            WindowsUiaBooleanCapabilityFact::Unknown,
            WindowsUiaBooleanCapabilityFact::Unknown,
        )
    }
}

impl WindowsUiaValueCapabilityFacts {
    pub const fn new(
        value_support: WindowsUiaPatternSupport,
        is_password: WindowsUiaBooleanCapabilityFact,
        is_read_only: WindowsUiaBooleanCapabilityFact,
    ) -> Self {
        Self {
            value_support,
            is_password,
            is_read_only,
        }
    }

    pub fn from_node(node: &NativeSemanticNodeObservation) -> Self {
        if !has_declared_capability_profile(node) {
            return Self::default();
        }

        let capabilities = WindowsUiaActionCapabilities::from_node(node);
        Self::new(
            capabilities.support_for(WindowsUiaPattern::Value),
            node.attributes
                .get(IS_PASSWORD_ATTRIBUTE_KEY)
                .map(|value| WindowsUiaBooleanCapabilityFact::from_wire_value(value))
                .unwrap_or_default(),
            node.attributes
                .get(VALUE_IS_READ_ONLY_ATTRIBUTE_KEY)
                .map(|value| WindowsUiaBooleanCapabilityFact::from_wire_value(value))
                .unwrap_or_default(),
        )
    }

    pub const fn value_support(self) -> WindowsUiaPatternSupport {
        self.value_support
    }

    pub const fn is_password(self) -> WindowsUiaBooleanCapabilityFact {
        self.is_password
    }

    pub const fn is_read_only(self) -> WindowsUiaBooleanCapabilityFact {
        self.is_read_only
    }

    pub const fn permits_set_value(self) -> bool {
        matches!(self.value_support, WindowsUiaPatternSupport::Supported)
            && matches!(self.is_password, WindowsUiaBooleanCapabilityFact::False)
            && matches!(
                self.is_read_only,
                WindowsUiaBooleanCapabilityFact::False
            )
    }

    pub fn write_attributes(self, attributes: &mut BTreeMap<String, String>) {
        attributes.insert(
            IS_PASSWORD_ATTRIBUTE_KEY.into(),
            self.is_password.as_wire_value().into(),
        );
        attributes.insert(
            VALUE_IS_READ_ONLY_ATTRIBUTE_KEY.into(),
            self.is_read_only.as_wire_value().into(),
        );
    }
}

fn has_declared_capability_profile(node: &NativeSemanticNodeObservation) -> bool {
    node.element_ref.provider_family == "windows_uia"
        && node
            .attributes
            .get(PROFILE_ATTRIBUTE_KEY)
            .map(String::as_str)
            == Some(WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1)
}
