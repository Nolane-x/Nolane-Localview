use std::fs;

fn source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).expect("source file must be readable")
}

fn function_body<'a>(source: &'a str, name: &str, next: &str) -> &'a str {
    let start = source.find(name).expect("function must exist");
    let tail = &source[start..];
    let end = tail.find(next).unwrap_or(tail.len());
    &tail[..end]
}

#[test]
fn wave9_candidate_is_bound_to_pending_fix_preimage_and_diff() {
    let trusted_fix = source("src/trusted_fix.rs");
    let body = function_body(
        &trusted_fix,
        "pub fn wave9_candidate_from_pending_proposal",
        "pub fn wave9_preflight_for_pending_proposal",
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
    assert!(body.contains("shadow_side_effect_containment"));
    assert!(body.contains("ProvenBlocked"));
    assert!(body.contains("unexpected_impact"));
    assert!(body.contains("stale_evidence_ids"));
}

#[test]
fn production_apply_authority_runs_wave9_shadow_preflight_before_applying() {
    let trusted_fix = source("src/trusted_fix.rs");
    let begin_apply = function_body(&trusted_fix, "pub fn begin_apply", "pub fn complete_apply");
    assert!(begin_apply.contains("wave9_preflight_for_pending_proposal(&pending)"));
    assert!(begin_apply.contains("ProductionCandidatePreflightVerdict::Rejected"));
    assert!(begin_apply.contains("project revision changed during candidate preflight"));
    assert!(begin_apply.contains("proposal.wave9_preflight = Some(preflight)"));
    assert!(begin_apply.contains("proposal.status = FixProposalStatus::Applying"));
}

#[test]
fn production_preflight_is_reachable_and_never_mints_verified() {
    let trusted_fix = source("src/trusted_fix.rs");
    let preflight = function_body(
        &trusted_fix,
        "pub fn wave9_preflight_for_pending_proposal",
        "pub fn validate_wave9_candidate_for_human_apply",
    );
    assert!(preflight.contains("exact_repository_revision"));
    assert!(preflight.contains("IsolationLevel::SemanticOnly"));
    assert!(preflight.contains("run_production_candidate_preflight"));
    assert!(preflight.contains("bind_production_affected_state"));
    assert!(preflight.contains("&proposal.canonical_route"));
    assert!(preflight.contains("Some(&proposal.reference)"));

    let verification = source("../../../crates/verification/src/autonomous.rs");

    assert!(verification.contains("compile_affected_state_plan"));
    assert!(verification.contains("dependency_graph_complete: false"));
    assert!(verification.contains("denominator_known: false"));
    assert!(verification.contains("affected_state_plan_hash"));
    assert!(verification.contains("predicted_impact"));
    let body = function_body(
        &verification,
        "pub fn run_production_candidate_preflight",
        "pub fn bind_production_affected_state",
    );
    assert!(body.contains("ShadowWorkspace::prepare"));
    assert!(body.contains("shadow.proof()"));
    assert!(body.contains("shadow.cleanup()"));
    assert!(body.contains("ExternalSideEffectContainment::ProvenBlocked"));
    assert!(body.contains("ProductionCandidatePreflightVerdict::Inconclusive"));
    assert!(!body.contains("AutonomousVerificationVerdict::Verified"));
}

#[test]
fn trusted_verify_wave9_recovery_preserves_previous_reader_compatibility() {
    let recovery = source("src/trusted_verify_recovery.rs");
    assert!(recovery.contains("const RECOVERY_SCHEMA_VERSION: u32 = 1;"));
    assert!(recovery.contains("struct PersistedWave9PreflightV1"));
    assert!(recovery.contains("format!(\"{id}.wave9\")"));

    let primary_writer = function_body(&recovery, "fn persisted_record(", "fn persist_record(");
    assert!(primary_writer.contains("-> PersistedVerificationRecordV1"));
    assert!(!primary_writer.contains("wave9_preflight:"));

    let persist = function_body(&recovery, "fn persist_record(", "fn consume_record(");
    let wave9_commit = persist
        .find("fs::rename(&wave9_temp, &wave9)")
        .expect("Wave 9 companion commit must exist");
    let primary_commit = persist
        .find("fs::rename(&temp, &meta)")
        .expect("primary metadata commit must exist");
    assert!(
        wave9_commit < primary_commit,
        "rollback-readable primary metadata must remain the final commit-point"
    );
}

#[test]
fn semantic_only_shadow_containment_has_explicit_fail_closed_controls() {
    let shadow = source("../../../crates/counterfactual/src/shadow.rs");
    assert!(shadow.contains("ExternalSideEffectContainment::ProvenBlocked"));
    assert!(shadow.contains("ExternalSideEffectContainment::NotProven"));
    assert!(shadow.contains("candidate.isolation != IsolationLevel::SemanticOnly"));
    assert!(shadow.contains("snapshot_visible_worktree"));
    assert!(shadow.contains("command.env_clear()"));
    assert!(shadow.contains("GIT_CONFIG_NOSYSTEM"));
    assert!(shadow.contains("GIT_ATTR_NOSYSTEM"));
    assert!(shadow.contains("core.hooksPath="));
    assert!(shadow.contains("core.fsmonitor=false"));
    assert!(shadow.contains("diff.external="));
    assert!(
        !shadow.contains("[\"status\", \"--porcelain=v1\"]"),
        "production containment proof must not execute repository-configurable git status"
    );
}
