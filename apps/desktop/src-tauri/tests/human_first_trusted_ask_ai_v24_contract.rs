fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start_index = source.find(start).expect("start marker must exist");
    let tail = &source[start_index..];
    let end_index = tail.find(end).expect("end marker must exist after start");
    &tail[..end_index]
}

#[test]
fn canonical_v24_spec_locks_trusted_ai_boundary() {
    let spec = include_str!("../../../../docs/superpowers/specs/2026-09-18-human-first-trusted-ask-ai-v24.md");

    for required in [
        "sessionId",
        "reference",
        "question",
        "Trusted context is backend-owned",
        "Provider secrets are backend-owned",
        "V2.4 is read-only",
        "Do not fake a connected provider",
        "source file contents are not included by default",
        "exact-head closure",
    ] {
        assert!(
            spec.contains(required),
            "canonical Ask AI V2.4 spec is missing boundary: {required}"
        );
    }
}

#[test]
fn frontend_ai_request_is_intent_only() {
    let api = include_str!("../../src/api.ts");

    assert!(api.contains("HumanAskAiRequest"));
    assert!(api.contains("askAiAboutSelection"));
    assert!(api.contains("'ask_ai_about_selection'"));
    assert!(api.contains("sessionId"));
    assert!(api.contains("reference"));
    assert!(api.contains("question"));

    let ask_api = between(api, "askAiAboutSelection", "measureElement");

    for forbidden in [
        "file:",
        "path:",
        "root:",
        "route:",
        "line:",
        "column:",
        "model:",
        "headers:",
        "apiKey:",
        "endpoint:",
        "systemPrompt:",
    ] {
        assert!(
            !ask_api.contains(forbidden),
            "frontend Ask AI request must not author {forbidden}"
        );
    }
}

#[test]
fn desktop_has_provider_neutral_trusted_ai_module() {
    let desktop = include_str!("../src/lib.rs");

    for required in [
        "mod trusted_ai;",
        "ask_ai_about_selection",
        "ai_provider_capability",
        "trusted_ai::validate_question",
        "trusted_ai::build_trusted_ai_context",
        "trusted_ai::ask_with_provider",
    ] {
        assert!(
            desktop.contains(required),
            "desktop trusted Ask AI authority is missing {required}"
        );
    }

    for forbidden in [
        "send_prompt_raw",
        "fetch_provider_from_frontend",
        "openai_api_key_from_frontend",
        "anthropic_api_key_from_frontend",
    ] {
        assert!(
            !desktop.contains(forbidden),
            "desktop Ask AI must not expose generic provider authority: {forbidden}"
        );
    }
}

#[test]
fn provider_secrets_do_not_live_in_frontend_storage() {
    let api = include_str!("../../src/api.ts");
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let preferences = include_str!("../../src/preferences.ts");

    let combined = format!("{api}\n{shell}\n{tools}\n{preferences}");
    for forbidden in [
        "aiApiKey",
        "providerApiKey",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "Authorization: Bearer",
        "localStorage.setItem('localview.ai",
    ] {
        assert!(
            !combined.contains(forbidden),
            "frontend must not own provider secret material: {forbidden}"
        );
    }
}

#[test]
fn trusted_context_policy_is_bounded_and_privacy_minimized() {
    let desktop = include_str!("../src/lib.rs");
    let trusted_ai = include_str!("../src/trusted_ai.rs");

    for required in [
        "MAX_AI_QUESTION_BYTES",
        "MAX_AI_CONTEXT_BYTES",
        "MAX_AI_NEARBY_NODES",
        "MAX_AI_CONSOLE_ISSUES",
        "MAX_AI_NETWORK_ISSUES",
        "AI_CONTEXT_VERSION",
        "trusted_attributes",
        "route_path_only",
        "safe_source_locator",
    ] {
        assert!(
            trusted_ai.contains(required),
            "trusted Ask AI context policy is missing {required}"
        );
    }

    for required in [
        "semantic-snapshot/fresh",
        "managed_surface_canonical_route",
        "pre_route",
        "post_route",
    ] {
        assert!(
            desktop.contains(required),
            "desktop Ask AI fresh authority is missing {required}"
        );
    }

    for forbidden in [
        "read_to_string(project_root)",
        "recursive_repository_scan",
        "attach_full_page_screenshot",
        "dangerouslySetInnerHTML",
    ] {
        assert!(
            !trusted_ai.contains(forbidden) && !desktop.contains(forbidden),
            "Ask AI V2.4 must not expand context authority: {forbidden}"
        );
    }
}

#[test]
fn ask_ai_is_reference_bound_and_shared_across_human_surfaces() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    assert!(shell.contains("HumanAskAiState"));
    assert!(shell.contains("askAiAboutSelection"));
    assert!(shell.contains("askAiGeneration"));
    assert!(shell.contains("selectedReference={selectedReference}"));
    assert!(shell.contains("case COMMAND_IDS.aiAskSelection:"));

    assert!(tools.contains("askAiState"));
    assert!(tools.contains("onAskAi"));
    assert!(tools.contains("COMMAND_IDS.aiAskSelection"));
    assert!(tools.contains("disabled: !current || !selectedReference"));
}

#[test]
fn ask_ai_v24_remains_read_only_when_fix_v25_is_present() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let desktop = include_str!("../src/lib.rs");
    let api = include_str!("../../src/api.ts");

    let shell_ask = between(
        shell,
        "const askAiAboutSelection = useCallback",
        "const beginFixReview = useCallback",
    );
    for forbidden in [
        "prepareFixProposal",
        "applyFixProposal",
        "discardFixProposal",
        "COMMAND_IDS.aiFixSelection",
    ] {
        assert!(
            !shell_ask.contains(forbidden),
            "Ask AI V2.4 lifecycle must remain read-only after V2.5: {forbidden}"
        );
    }

    let desktop_ask = between(
        desktop,
        "async fn ask_ai_about_selection(",
        "async fn prepare_fix_proposal(",
    );
    for forbidden in [
        "trusted_fix::",
        "prepare_fix_proposal",
        "apply_fix_proposal",
        "fs::write",
        "fs::rename",
    ] {
        assert!(
            !desktop_ask.contains(forbidden),
            "Ask AI V2.4 backend must not gain Fix/write authority: {forbidden}"
        );
    }

    let api_ask = between(api, "askAiAboutSelection", "prepareFixProposal");
    for forbidden in [
        "prepare_fix_proposal",
        "apply_fix_proposal",
        "proposalId",
        "replacement",
        "diff",
    ] {
        assert!(
            !api_ask.contains(forbidden),
            "Ask AI V2.4 API must remain intent-only after V2.5: {forbidden}"
        );
    }
}

#[test]
fn ask_ai_failure_is_humanized_and_localized() {
    let i18n = include_str!("../../src/i18n.ts");
    let tools = include_str!("../../src/features/FloatingTools.tsx");

    for key in [
        "ai.providerConnected",
        "ai.unavailable",
        "ai.askSelection",
        "ai.ask",
        "ai.asking",
        "ai.answer",
        "ai.enterQuestion",
        "ai.questionTooLong",
        "ai.contextUnavailable",
        "ai.failed",
        "ai.advisory",
    ] {
        assert!(
            i18n.contains(&format!("'{key}'")),
            "missing Ask AI localization key {key}"
        );
    }

    assert!(tools.contains("translate(locale, 'ai.asking')"));
    assert!(tools.contains("translate(locale, 'ai.failed')"));
}

#[test]
fn render_audit_proves_trusted_ask_ai_runtime_states() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "64-ai-provider-unavailable.png",
        "65-ai-ready-selection.png",
        "66-ai-no-selection.png",
        "67-ai-no-session.png",
        "68-ai-empty-question.png",
        "69-ai-oversized-question.png",
        "70-ai-asking.png",
        "71-ai-success.png",
        "72-ai-provider-failure.png",
        "73-ai-context-unavailable.png",
        "74-ai-stale-selection.png",
        "75-ai-stale-session.png",
        "76-vi-ai-unavailable.png",
        "77-vi-ai-success.png",
        "78-ai-narrow-success.png",
        "79-ai-failure-isolation.png",
        "80-ai-command-no-selection.png",
        "81-ai-command-provider-unavailable.png",
        "82-ai-command-success.png",
        "83-ai-fix-remains-disabled.png",
    ] {
        assert!(
            capture.contains(artifact),
            "trusted Ask AI render audit is missing {artifact}"
        );
    }

    for marker in [
        "ask_ai_about_selection",
        "ai:request-intent-only",
        "ai:no-caller-context-authority",
        "ai:provider-secret-not-visible",
        "ai:duplicate-suppressed",
        "ai:no-raw-error",
        "ai:stale-selection-isolated",
        "ai:stale-session-isolated",
        "ai:command-shared-request",
        "ai:fix-remains-disabled",
        "ai:route-query-redacted",
    ] {
        assert!(
            capture.contains(marker),
            "trusted Ask AI render audit is missing executable marker {marker}"
        );
    }
}
