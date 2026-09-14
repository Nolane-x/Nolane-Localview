use localview_macos_ax_provider::{
    AxSensitiveTextMetadata, AxSensitiveTextPolicy, AxSensitiveTextProtection, AxSensitiveTextTaint,
};

#[test]
fn secure_text_subrole_is_opaque_and_never_value_readable() {
    let decision = AxSensitiveTextPolicy::classify(AxSensitiveTextMetadata::new(
        "AXTextField",
        Some("AXSecureTextField"),
    ));

    assert_eq!(
        decision.protection(),
        AxSensitiveTextProtection::ProtectedSecureText
    );
    assert_eq!(decision.semantic_text_protection(), Some("protected_secure_text"));
    assert!(!decision.value_read_permitted());
    assert!(decision.taints().contains(&AxSensitiveTextTaint::Secret));
    assert!(decision.taints().contains(&AxSensitiveTextTaint::Credential));
}

#[test]
fn ordinary_text_field_is_not_falsely_promoted_to_secret() {
    let decision = AxSensitiveTextPolicy::classify(AxSensitiveTextMetadata::new(
        "AXTextField",
        None,
    ));

    assert_eq!(decision.protection(), AxSensitiveTextProtection::Ordinary);
    assert_eq!(decision.semantic_text_protection(), None);
    assert!(decision.value_read_permitted());
    assert!(decision.taints().is_empty());
}
