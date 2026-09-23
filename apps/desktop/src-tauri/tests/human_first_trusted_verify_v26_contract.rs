fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_v26_spec_locks_read_only_verify_boundary() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-19-human-first-trusted-verify-v26.md");

    for required in [
        "verificationId",
        "minted during the trusted Apply lifecycle",
        "V2.6 is read-only",
        "No automatic rollback",
        "Semantic baseline is mandatory",
        "VisualBaselineCache",
        "change_observed",
        "no_observable_change",
        "regression_signal",
        "inconclusive",
        "VERIFY_CONTEXT_VERSION = 1",
        "exact-head closure",
    ] {
        assert!(
            spec.contains(required),
            "canonical Trusted Verify V2.6 spec is missing boundary: {required}"
        );
    }
}

#[test]
fn apply_frontend_authority_remains_proposal_id_only() {
    let api = include_str!("../../src/api.ts");

    let apply = between(api, "applyFixProposal", "discardFixProposal");
    assert!(apply.contains("proposalId"));

    for forbidden in [
        "sessionId",
        "reference",
        "path:",
        "file:",
        "route:",
        "viewport:",
        "rect:",
        "baseline",
        "evidenceId",
        "preimage",
        "postimage",
        "verificationId",
    ] {
        assert!(
            !apply.contains(forbidden),
            "V2.6 must not expand frontend Apply authority with {forbidden}"
        );
    }
}

#[test]
fn verify_frontend_request_is_verification_id_only() {
    let api = include_str!("../../src/api.ts");

    assert!(api.contains("HumanVerifyChangeRequest"));
    assert!(api.contains("verifyFixChange"));
    assert!(api.contains("'verify_fix_change'"));

    let verify = between(api, "verifyFixChange", "captureCurrentViewport");
    assert!(verify.contains("verificationId"));

    for forbidden in [
        "sessionId",
        "reference",
        "path:",
        "file:",
        "route:",
        "viewport:",
        "rect:",
        "baseline",
        "evidenceId",
        "source",
        "preimage",
        "postimage",
        "rollback",
    ] {
        assert!(
            !verify.contains(forbidden),
            "frontend Verify must not author {forbidden}"
        );
    }
}

#[test]
fn apply_receipt_exposes_opaque_verification_identity_and_scope() {
    let api = include_str!("../../src/api.ts");
    let fix = include_str!("../src/trusted_fix.rs");

    for required in [
        "verificationId",
        "verificationScope",
        "semantic_visual",
        "semantic_only",
    ] {
        assert!(
            api.contains(required) || fix.contains(required),
            "Apply receipt is missing V2.6 verification metadata: {required}"
        );
    }
}

#[test]
fn desktop_has_isolated_trusted_verify_module_and_exact_command() {
    let desktop = include_str!("../src/lib.rs");
    let permissions = include_str!("../permissions/localview.toml");

    for required in [
        "mod trusted_verify;",
        "verify_fix_change",
        "trusted_verify::VerificationStore",
        "trusted_verify::VERIFY_CONTEXT_VERSION",
    ] {
        assert!(
            desktop.contains(required),
            "desktop Trusted Verify boundary is missing {required}"
        );
    }

    assert!(permissions.contains(r#""verify_fix_change""#));

    for forbidden in [
        "rollback_fix_change",
        "write_verified_file",
        "apply_verify_patch",
        "verify_shell_command",
    ] {
        assert!(
            !desktop.contains(forbidden),
            "Verify must not expose mutation authority: {forbidden}"
        );
    }
}

#[test]
fn verification_store_is_bounded_versioned_and_backend_owned() {
    let verify = include_str!("../src/trusted_verify.rs");

    for required in [
        "VERIFY_CONTEXT_VERSION",
        "MAX_VERIFICATION_RECORDS",
        "VERIFICATION_TTL",
        "MAX_VERIFY_VISUAL_BYTES_PER_RECORD",
        "MAX_VERIFY_TOTAL_VISUAL_BYTES",
        "MAX_VERIFY_SEMANTIC_BYTES",
        "VerificationStore",
        "VerificationRecord",
        "VerificationStatus",
        "Pending",
        "Verifying",
        "Verified",
        "Expired",
        "Invalidated",
        "reap_expired",
    ] {
        assert!(
            verify.contains(required),
            "Trusted Verify store/policy is missing {required}"
        );
    }
}

#[test]
fn apply_mints_baseline_before_write_and_cleans_it_on_failure() {
    let desktop = include_str!("../src/lib.rs");

    let apply = between(
        desktop,
        "async fn apply_fix_proposal(",
        "fn discard_fix_proposal(",
    );

    for required in [
        "mint_verification_baseline",
        "apply_fix_transaction",
        "discard_verification",
    ] {
        assert!(
            apply.contains(required),
            "V2.6 Apply lifecycle is missing {required}"
        );
    }

    let mint = apply
        .find("mint_verification_baseline")
        .expect("verification baseline mint must exist");
    let write = apply
        .find("apply_fix_transaction")
        .expect("Fix write transaction must exist");
    assert!(
        mint < write,
        "verification baseline must be minted before source mutation"
    );

    assert!(
        apply.contains("verification_id"),
        "Apply receipt must bind the minted verification record"
    );
}

#[test]
fn verify_revalidates_postimage_route_session_and_exact_reference() {
    let desktop = include_str!("../src/lib.rs");

    let verify = between(
        desktop,
        "async fn verify_fix_change(",
        "async fn open_source_for_selection(",
    );

    for required in [
        "semantic-snapshot/fresh",
        "managed_surface_canonical_route",
        "postimage",
        "resolve_snapshot_source",
        "resolve_trusted_source_target",
        "reference",
        "canonical_route",
    ] {
        assert!(
            verify.contains(required),
            "Verify fresh authority is missing {required}"
        );
    }

    for forbidden in [
        "apply_fix_transaction",
        "fs::write",
        "fs::rename",
        "rollback",
        "prepare_fix_proposal",
    ] {
        assert!(
            !verify.contains(forbidden),
            "Verify must remain read-only: {forbidden}"
        );
    }
}

#[test]
fn trusted_verify_compares_semantics_issues_and_redacted_visual_facts() {
    let verify = include_str!("../src/trusted_verify.rs");
    let visual = include_str!("../src/visual_capture.rs");

    for required in [
        "build_semantic_baseline",
        "compare_semantic_projection",
        "issue_fingerprint",
        "compare_issue_fingerprints",
        "classify_verification_status",
        "viewport_changed_ratio",
        "target_changed_ratio",
        "pixel_diff",
        "change_observed",
        "no_observable_change",
        "regression_signal",
        "inconclusive",
    ] {
        assert!(
            verify.contains(required),
            "deterministic Verify comparison is missing {required}"
        );
    }

    for required in [
        "capture_verification_baseline",
        "capture_verification_current",
        "redact_private_pixels",
        "trusted_viewport_from_freeze",
    ] {
        assert!(
            visual.contains(required),
            "trusted visual Verify path is missing {required}"
        );
    }

    assert!(
        !verify.contains("VisualBaselineCache"),
        "V2.6 record must not use the session-global baseline cache as its proof baseline"
    );
}

#[test]
fn provider_assessment_is_advisory_and_cannot_override_deterministic_status() {
    let verify = include_str!("../src/trusted_verify.rs");
    let desktop = include_str!("../src/lib.rs");

    for required in [
        "deterministic_status",
        "advisory_summary",
        "provider_label",
    ] {
        assert!(
            verify.contains(required),
            "provider advisory separation is missing {required}"
        );
    }

    let verify_command = between(
        desktop,
        "async fn verify_fix_change(",
        "async fn open_source_for_selection(",
    );
    for required in [
        "trusted_ai::provider_config_from_env()",
        "trusted_ai::ask_with_provider(",
        "Duration::from_secs(2)",
        "Advisory only.",
        "Do not override or relabel the deterministic status.",
        "receipt.provider_label = Some(answer.provider_label)",
        "receipt.advisory_summary = Some(answer.answer)",
    ] {
        assert!(
            verify_command.contains(required),
            "production Verify advisory path is missing {required}"
        );
    }
    let receipt_status = verify_command
        .find("status: comparison.deterministic_status")
        .expect("deterministic receipt status must be assigned");
    let provider_call = verify_command
        .find("trusted_ai::ask_with_provider(")
        .expect("optional provider advisory must be reachable");
    assert!(
        receipt_status < provider_call,
        "provider advisory must execute only after deterministic status is fixed"
    );

    for forbidden in [
        "provider_status_override",
        "provider_can_verify",
        "automatic_rollback",
        "provider_patch",
    ] {
        assert!(
            !verify.contains(forbidden) && !verify_command.contains(forbidden),
            "provider must not gain deterministic/write authority: {forbidden}"
        );
    }
}

#[test]
fn human_verify_state_is_reference_session_and_verification_bound() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let commands = include_str!("../../src/commands.ts");

    for required in [
        "HumanVerifyState",
        "verifyGeneration",
        "verificationId",
        "verifyFixChange",
        "verifyCanRetry",
        "settle_failed",
        "COMMAND_IDS.aiVerifyChange",
    ] {
        assert!(
            shell.contains(required) || tools.contains(required) || commands.contains(required),
            "Human-First Verify UI is missing {required}"
        );
    }

    assert!(commands.contains("aiVerifyChange"));
    assert!(tools.contains("verifyState"));
    assert!(tools.contains("onVerifyChange"));
}

#[test]
fn verify_is_localized_and_truthful_about_scope() {
    let i18n = include_str!("../../src/i18n.ts");

    for key in [
        "verify.title",
        "verify.ready",
        "verify.action",
        "verify.inProgress",
        "verify.changeObserved",
        "verify.noObservableChange",
        "verify.regressionSignal",
        "verify.inconclusive",
        "verify.semanticVisual",
        "verify.semanticOnly",
        "verify.expired",
        "verify.sourceChanged",
        "verify.routeChanged",
        "verify.targetUnavailable",
        "verify.failed",
        "verify.objectiveFacts",
        "verify.aiAssessment",
        "verify.readOnlyDisclosure",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing Trusted Verify localization key {key}"
        );
    }
}

#[test]
fn render_audit_proves_trusted_verify_runtime_matrix() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "116-verify-ready.png",
        "117-verify-semantic-visual-scope.png",
        "118-verify-semantic-only-scope.png",
        "119-verify-in-flight.png",
        "120-verify-duplicate-suppressed.png",
        "121-verify-change-observed.png",
        "122-verify-no-observable-change.png",
        "123-verify-regression-signal.png",
        "124-verify-inconclusive.png",
        "125-verify-expired.png",
        "126-verify-source-changed.png",
        "127-verify-route-changed.png",
        "128-verify-target-unavailable.png",
        "129-verify-console-regression.png",
        "130-verify-network-regression.png",
        "131-verify-provider-advisory.png",
        "132-verify-provider-unavailable-deterministic.png",
        "133-verify-provider-error-hidden.png",
        "134-verify-stale-selection.png",
        "135-verify-stale-session.png",
        "136-verify-narrow-result.png",
        "137-vi-verify-ready.png",
        "138-vi-verify-change-observed.png",
        "139-verify-failure-isolation.png",
        "140-verify-command-unavailable.png",
        "141-verify-command-shared-request.png",
        "142-verify-request-id-only.png",
        "143-verify-no-caller-authority.png",
        "144-verify-no-auto-rollback.png",
        "145-fix-after-verify.png",
    ] {
        assert!(
            capture.contains(artifact),
            "Trusted Verify render audit is missing {artifact}"
        );
    }

    for marker in [
        "verify_fix_change",
        "verify:request-id-only",
        "verify:no-caller-path-authority",
        "verify:no-caller-reference-authority",
        "verify:no-caller-viewport-authority",
        "verify:no-caller-evidence-authority",
        "verify:duplicate-suppressed",
        "verify:change-observed",
        "verify:no-observable-change",
        "verify:regression-signal",
        "verify:inconclusive",
        "verify:provider-advisory-separate",
        "verify:no-raw-error",
        "verify:retryable-failure-action-visible",
        "verify:command-retry-enabled",
        "verify:retryable-failure-retry",
        "verify:stale-selection-isolated",
        "verify:stale-session-isolated",
        "verify:command-shared-request",
        "verify:no-auto-rollback",
    ] {
        assert!(
            capture.contains(marker),
            "Trusted Verify render audit is missing executable marker {marker}"
        );
    }
}

#[test]
fn verify_recovery_is_durable_hash_bound_and_revalidated_after_restart() {
    let verify = include_str!("../src/trusted_verify.rs");
    let recovery = include_str!("../src/trusted_verify_recovery.rs");
    let desktop = include_str!("../src/lib.rs");

    assert!(desktop.contains("VerificationStore::production"));
    assert!(verify.contains("postimage_sha256"));
    assert!(!verify.contains("pub postimage: Vec<u8>"));
    assert!(recovery.contains("RECOVERY_SCHEMA_VERSION"));
    assert!(recovery.contains("expires_at_unix_ms"));
    assert!(recovery.contains("png_sha256"));
    assert!(recovery.contains("VerificationStatus::Pending"));
    assert!(recovery.contains("fs::rename(&meta, &consumed)"));

    let command = between(
        desktop,
        "async fn verify_fix_change(",
        "async fn open_source_for_selection(",
    );
    assert!(command.contains("managed_surface_canonical_route"));
    assert!(command.contains("semantic-snapshot/fresh"));
    assert!(command.contains("resolve_trusted_source_target"));
    assert!(command.contains("sha256_bytes(&postimage) != record.postimage_sha256"));
}
