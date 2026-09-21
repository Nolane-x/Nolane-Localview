use std::fs;

fn source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).expect("source file must be readable")
}

fn function_body<'a>(source: &'a str, name: &str, next: &str) -> &'a str {
    let start = source.find(name).expect("function must exist");
    let tail = &source[start..];
    let end = tail
        .find(next)
        .unwrap_or(tail.len());
    &tail[..end]
}

#[test]
fn wave9_candidate_is_bound_to_pending_fix_preimage_and_diff() {
    let trusted_fix = source("src/trusted_fix.rs");
    let body = function_body(
        &trusted_fix,
        "pub fn wave9_candidate_from_pending_proposal",
        "pub fn validate_wave9_candidate_for_human_apply",
    );
    assert!(body.contains("proposal.status != FixProposalStatus::Pending"));
    assert!(body.contains("current != proposal.preimage"));
    assert!(body.contains("sha256_bytes(&proposal.preimage)"));
    assert!(body.contains("patch: proposal.diff.clone()"));
    assert!(body.contains("disposable: true"));
}

#[test]
fn wave9_handoff_cannot_apply_or_bypass_human_authority() {
    let trusted_fix = source("src/trusted_fix.rs");
    let body = function_body(
        &trusted_fix,
        "pub fn validate_wave9_candidate_for_human_apply",
        "#[cfg(test)]",
    );
    assert!(body.contains("validate_wave9_verified_handoff"));
    assert!(body.contains("candidate.id.to_string() != receipt.candidate_id"));
    assert!(body.contains("patch_digest(&candidate.overlays) != receipt.patch_digest"));
    assert!(body.contains("current != proposal.preimage"));
    assert!(!body.contains("begin_apply("));
    assert!(!body.contains("apply_fix_transaction("));
    assert!(!body.contains("fs::write("));
}

#[test]
fn trusted_verify_requires_verified_complete_clean_receipt() {
    let trusted_verify = source("src/trusted_verify.rs");
    let body = function_body(
        &trusted_verify,
        "pub fn validate_wave9_verified_handoff",
        "#[cfg(test)]",
    );
    assert!(body.contains("receipt.base_revision != expected_base_revision"));
    assert!(body.contains("AutonomousVerificationVerdict::Verified"));
    assert!(body.contains("receipt.cleanup_proof.complete()"));
    assert!(body.contains("receipt.resource_budget.within_budget()"));
    assert!(body.contains("hard_unknowns"));
    assert!(body.contains("surviving_mutations"));
    assert!(body.contains("unexpected_impact"));
    assert!(body.contains("stale_evidence_ids"));
}
