use std::fs;

use localview_windows_uia_provider::{
    WindowsUiaVerifiedInputReceipt, WindowsUiaVerifiedInputRequest,
};

#[test]
fn raw_verified_input_request_fields_are_not_publicly_constructible() {
    let source_path = format!("{}/src/verified_input.rs", env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(source_path).expect("read verified_input.rs");
    let start = source
        .find("pub struct WindowsUiaVerifiedInputRequest {")
        .expect("verified-input request must exist");
    let tail = &source[start..];
    let end = tail
        .find("\n}\n")
        .expect("verified-input request body must close");
    let request_body = &tail[..end];

    for forbidden in [
        "pub dispatch_attempt_ref:",
        "pub action_id:",
        "pub preparation_journal_sequence:",
        "pub preparation_receipt_ref:",
        "pub snapshot_cut_ref:",
        "pub provider_incarnation_ref:",
        "pub target_incarnation_ref:",
        "pub element_ref:",
        "pub context_requirements:",
        "pub batch_digest:",
        "pub batch:",
    ] {
        assert!(
            !request_body.contains(forbidden),
            "verified-input request leaks publicly constructible authority field: {forbidden}"
        );
    }

    assert!(
        source.contains("DispatchExecutionPermit"),
        "verified-input provider request must be gated by the journal-minted one-shot permit"
    );
    assert!(
        source.contains("from_execution_permit"),
        "verified-input provider request must expose only an authority-gated constructor"
    );
}

#[test]
fn verified_input_receipt_exposes_exact_authority_and_batch_binding() {
    fn assert_binding_api(
        receipt: &WindowsUiaVerifiedInputReceipt,
        request: &WindowsUiaVerifiedInputRequest,
    ) {
        assert_eq!(
            receipt.dispatch_attempt_ref(),
            request.dispatch_attempt_ref()
        );
        assert_eq!(receipt.action_id(), request.action_id());
        assert_eq!(
            receipt.preparation_journal_sequence(),
            request.preparation_journal_sequence()
        );
        assert_eq!(
            receipt.preparation_receipt_ref(),
            request.preparation_receipt_ref()
        );
        assert_eq!(receipt.snapshot_cut_ref(), request.snapshot_cut_ref());
        assert_eq!(
            receipt.provider_incarnation_ref(),
            request.provider_incarnation_ref()
        );
        assert_eq!(
            receipt.target_incarnation_ref(),
            request.target_incarnation_ref()
        );
        assert_eq!(receipt.element_ref(), request.element_ref());
        assert_eq!(receipt.batch_digest(), request.batch_digest());
        assert!(receipt.boundary().reconciliation_required);
    }

    let _ =
        assert_binding_api as fn(&WindowsUiaVerifiedInputReceipt, &WindowsUiaVerifiedInputRequest);
}
