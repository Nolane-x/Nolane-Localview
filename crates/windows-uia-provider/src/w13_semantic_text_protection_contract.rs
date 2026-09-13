#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{WindowsUiaBooleanCapabilityFact, w13_semantic_text_protection::write_semantic_text_protection};

    #[test]
    fn only_explicit_password_true_emits_protected_password_fact() {
        for (fact, expected) in [
            (WindowsUiaBooleanCapabilityFact::True, Some("protected_password")),
            (WindowsUiaBooleanCapabilityFact::False, None),
            (WindowsUiaBooleanCapabilityFact::Unknown, None),
        ] {
            let mut attributes = BTreeMap::new();
            write_semantic_text_protection(fact, &mut attributes);
            assert_eq!(
                attributes
                    .get("windows_uia.semantic_text_protection")
                    .map(String::as_str),
                expected
            );
        }
    }
}
