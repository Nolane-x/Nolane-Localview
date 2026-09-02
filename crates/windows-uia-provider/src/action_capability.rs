use std::collections::BTreeMap;

use localview_native_provider::NativeSemanticNodeObservation;

pub const WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1: &str =
    "windows-uia-action-capabilities-v1";
const PROFILE_ATTRIBUTE_KEY: &str = "windows_uia.action_capability_profile";

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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowsUiaActionCapabilities {
    patterns: BTreeMap<WindowsUiaPattern, WindowsUiaPatternSupport>,
}

impl WindowsUiaActionCapabilities {
    pub const fn profile_attribute_key() -> &'static str {
        PROFILE_ATTRIBUTE_KEY
    }

    pub fn from_node(node: &NativeSemanticNodeObservation) -> Self {
        if node.element_ref.provider_family != "windows_uia"
            || node
                .attributes
                .get(PROFILE_ATTRIBUTE_KEY)
                .map(String::as_str)
                != Some(WINDOWS_UIA_ACTION_CAPABILITY_PROFILE_V1)
        {
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
