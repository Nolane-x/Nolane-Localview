use std::collections::BTreeMap;

use crate::WindowsUiaBooleanCapabilityFact;

pub(crate) const SEMANTIC_TEXT_PROTECTION_ATTRIBUTE_KEY: &str =
    "windows_uia.semantic_text_protection";

pub(crate) fn write_semantic_text_protection(
    is_password: WindowsUiaBooleanCapabilityFact,
    attributes: &mut BTreeMap<String, String>,
) {
    if matches!(is_password, WindowsUiaBooleanCapabilityFact::True) {
        attributes.insert(
            SEMANTIC_TEXT_PROTECTION_ATTRIBUTE_KEY.into(),
            "protected_password".into(),
        );
    }
}
