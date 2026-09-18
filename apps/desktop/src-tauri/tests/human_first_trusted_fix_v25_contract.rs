fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_v25_spec_locks_two_phase_write_boundary() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-19-human-first-trusted-fix-v25.md");

    for required in [
        "proposal is not permission to write",
        "Two-phase authority",
        "sessionId",
        "reference",
        "instruction",
        "proposalId",
        "one file, one contiguous edit",
        "exact preimage",
        "5 minutes",
        "No force apply",
        "transactional write",
        "rollback",
        "Do not mark the PR ready",
    ] {
        assert!(
            spec.contains(required),
            "canonical Trusted Fix V2.5 spec is missing boundary: {required}"
        );
    }
}

#[test]
fn frontend_prepare_request_is_intent_only_and_apply_is_opaque_id_only() {
    let api = include_str!("../../src/api.ts");

    assert!(api.contains("HumanFixProposalRequest"));
    assert!(api.contains("prepareFixProposal"));
    assert!(api.contains("'prepare_fix_proposal'"));
    assert!(api.contains("sessionId"));
    assert!(api.contains("reference"));
    assert!(api.contains("instruction"));

    let prepare = between(api, "prepareFixProposal", "applyFixProposal");
    for forbidden in [
        "file:",
        "path:",
        "root:",
        "route:",
        "line:",
        "column:",
        "replacement:",
        "diff:",
        "model:",
        "headers:",
        "endpoint:",
    ] {
        assert!(
            !prepare.contains(forbidden),
            "frontend Fix proposal request must not author {forbidden}"
        );
    }

    assert!(api.contains("applyFixProposal"));
    assert!(api.contains("'apply_fix_proposal'"));
    let apply = between(api, "applyFixProposal", "discardFixProposal");
    assert!(apply.contains("proposalId"));
    for forbidden in [
        "file:",
        "path:",
        "reference:",
        "replacement:",
        "diff:",
        "force:",
        "ignoreStale:",
    ] {
        assert!(
            !apply.contains(forbidden),
            "frontend Apply must not author {forbidden}"
        );
    }
}

#[test]
fn desktop_exposes_only_exact_fix_commands() {
    let desktop = include_str!("../src/lib.rs");

    for required in [
        "mod trusted_fix;",
        "ai_fix_capability",
        "prepare_fix_proposal",
        "apply_fix_proposal",
        "discard_fix_proposal",
    ] {
        assert!(
            desktop.contains(required),
            "desktop Trusted Fix boundary is missing {required}"
        );
    }

    for forbidden in [
        "write_file_from_frontend",
        "apply_patch_from_frontend",
        "save_text_from_frontend",
        "run_fix_shell",
        "force_apply_fix",
    ] {
        assert!(
            !desktop.contains(forbidden),
            "Trusted Fix must not expose generic write authority: {forbidden}"
        );
    }
}

#[test]
fn trusted_fix_core_is_bounded_one_file_one_edit_and_strict() {
    let fix = include_str!("../src/trusted_fix.rs");

    for required in [
        "MAX_FIX_INSTRUCTION_BYTES",
        "MAX_FIX_FILE_BYTES",
        "MAX_FIX_SOURCE_EXCERPT_BYTES",
        "MAX_FIX_REPLACEMENT_BYTES",
        "MAX_FIX_DIFF_BYTES",
        "FIX_PROPOSAL_TTL",
        "MAX_FIX_PROPOSALS",
        "FixProviderEdit",
        "start_line",
        "end_line",
        "replacement",
        "deny_unknown_fields",
        "validate_fix_instruction",
        "validate_fix_source_policy",
        "build_source_excerpt",
        "build_fix_postimage",
        "build_fix_diff",
    ] {
        assert!(
            fix.contains(required),
            "Trusted Fix core is missing {required}"
        );
    }

    for forbidden in [
        "Vec<FixProviderEdit>",
        "files:",
        "target_path:",
        "shell_command:",
        "git apply",
        "sh -c",
        "cmd /C",
        "powershell -Command",
    ] {
        assert!(
            !fix.contains(forbidden),
            "V2.5 must remain one-file/one-edit and shell-free: {forbidden}"
        );
    }
}

#[test]
fn fix_write_target_is_stricter_than_open_source() {
    let fix = include_str!("../src/trusted_fix.rs");

    for required in [
        "reject_symlink_path_components",
        "symlink_metadata",
        "SUPPORTED_FIX_EXTENSIONS",
        "SENSITIVE_FIX_BASENAMES",
        "is_file",
        "canonicalize",
        "starts_with",
        "valid UTF-8",
    ] {
        assert!(
            fix.contains(required),
            "Trusted Fix write target policy is missing {required}"
        );
    }
}

#[test]
fn proposal_store_is_backend_owned_bounded_ttl_and_one_shot() {
    let fix = include_str!("../src/trusted_fix.rs");

    for required in [
        "FixProposalStore",
        "FixProposalRecord",
        "proposal_id",
        "preimage",
        "postimage",
        "expires_at",
        "Pending",
        "Applying",
        "Applied",
        "Discarded",
        "Invalidated",
        "reap_expired",
        "MAX_FIX_PROPOSALS",
    ] {
        assert!(
            fix.contains(required),
            "proposal store is missing {required}"
        );
    }
}

#[test]
fn apply_revalidates_fresh_authority_and_exact_preimage() {
    let desktop = include_str!("../src/lib.rs");
    let fix = include_str!("../src/trusted_fix.rs");

    for required in [
        "semantic-snapshot/fresh",
        "managed_surface_canonical_route",
        "resolve_snapshot_source",
        "resolve_trusted_source_target",
        "proposal.preimage",
        "current_bytes",
        "source changed",
    ] {
        assert!(
            desktop.contains(required) || fix.contains(required),
            "Apply stale-authority closure is missing {required}"
        );
    }

    assert!(
        !desktop.contains("force_apply_fix"),
        "V2.5 must not expose force apply"
    );
}

#[test]
fn transaction_has_backend_owned_temp_backup_verification_and_rollback() {
    let fix = include_str!("../src/trusted_fix.rs");

    for required in [
        "apply_fix_transaction",
        "create_new",
        "sync_all",
        "permissions",
        "backup",
        "postimage",
        "rollback",
        "remove_file",
    ] {
        assert!(
            fix.contains(required),
            "Trusted Fix transaction is missing {required}"
        );
    }

    for forbidden in [
        "Command::new(\\"git\\")",
        "Command::new(\\"patch\\")",
        "Command::new(\\"sh\\")",
        "Command::new(\\"cmd\\")",
        "Command::new(\\"powershell\\")",
    ] {
        assert!(
            !fix.contains(forbidden),
            "Trusted Fix transaction must not use shell/tool mutation: {forbidden}"
        );
    }
}

#[test]
fn provider_proposal_has_no_path_or_write_authority() {
    let fix = include_str!("../src/trusted_fix.rs");
    let ai = include_str!("../src/trusted_ai.rs");

    for required in [
        "fix_proposal",
        "source_excerpt",
        "provider_label",
    ] {
        assert!(
            fix.contains(required) || ai.contains(required),
            "Fix provider bridge is missing {required}"
        );
    }

    assert!(
        !fix.contains("provider_path"),
        "provider must not own target path"
    );
}

#[test]
fn human_fix_state_is_review_first_and_reference_bound() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    for required in [
        "HumanFixState",
        "fixGeneration",
        "prepareFixProposal",
        "applyFixProposal",
        "discardFixProposal",
        "proposalId",
        "selectedReference",
        "COMMAND_IDS.aiFixSelection",
    ] {
        assert!(
            shell.contains(required) || tools.contains(required),
            "Human-First Fix UI is missing {required}"
        );
    }

    assert!(
        tools.contains("fix-disclosure"),
        "Fix must disclose bounded source sharing before proposal generation"
    );
    assert!(
        tools.contains("fix-diff"),
        "Fix must show a diff before Apply"
    );
    assert!(
        tools.contains("fix-apply-action"),
        "Fix must have a separate Apply action"
    );
}

#[test]
fn fix_is_localized_and_verify_remains_separate() {
    let i18n = include_str!("../../src/i18n.ts");
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    for key in [
        "fix.unavailable",
        "fix.disclosure",
        "fix.instruction",
        "fix.generate",
        "fix.generating",
        "fix.review",
        "fix.apply",
        "fix.applying",
        "fix.discard",
        "fix.applied",
        "fix.expired",
        "fix.sourceChanged",
        "fix.failed",
        "fix.noWriteBeforeApply",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing Trusted Fix localization key {key}"
        );
    }

    assert!(
        !shell.contains("case COMMAND_IDS.aiVerifyChange:"),
        "V2.5 must not silently wire Verify Change"
    );
    assert!(tools.contains("ai.verifyChange"));
}

#[test]
fn render_audit_proves_trusted_fix_runtime_states() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "84-fix-provider-unavailable.png",
        "85-fix-disclosure.png",
        "86-fix-ready-selection.png",
        "87-fix-no-selection.png",
        "88-fix-no-session.png",
        "89-fix-empty-instruction.png",
        "90-fix-oversized-instruction.png",
        "91-fix-proposing.png",
        "92-fix-proposal-success.png",
        "93-fix-diff-visible.png",
        "94-fix-provider-failure.png",
        "95-fix-source-unavailable.png",
        "96-fix-sensitive-source-refused.png",
        "97-fix-unsupported-extension.png",
        "98-fix-stale-selection.png",
        "99-fix-stale-session.png",
        "100-fix-apply-ready.png",
        "101-fix-applying.png",
        "102-fix-apply-success.png",
        "103-fix-source-changed.png",
        "104-fix-route-changed.png",
        "105-fix-expired.png",
        "106-fix-transaction-failure.png",
        "107-fix-discard.png",
        "108-vi-fix-disclosure.png",
        "109-vi-fix-proposal.png",
        "110-vi-fix-apply-success.png",
        "111-fix-narrow-review.png",
        "112-fix-failure-isolation.png",
        "113-fix-command-no-selection.png",
        "114-fix-command-unavailable.png",
        "115-fix-command-review-flow.png",
    ] {
        assert!(
            capture.contains(artifact),
            "Trusted Fix render audit is missing {artifact}"
        );
    }

    for marker in [
        "prepare_fix_proposal",
        "apply_fix_proposal",
        "discard_fix_proposal",
        "fix:prepare-intent-only",
        "fix:no-caller-path-authority",
        "fix:no-caller-replacement-authority",
        "fix:duplicate-proposal-suppressed",
        "fix:apply-proposal-id-only",
        "fix:duplicate-apply-suppressed",
        "fix:no-auto-apply",
        "fix:stale-selection-isolated",
        "fix:stale-session-isolated",
        "fix:no-raw-error",
        "fix:discard-no-write",
        "fix:command-shared-review-flow",
    ] {
        assert!(
            capture.contains(marker),
            "Trusted Fix render audit is missing executable marker {marker}"
        );
    }
}
