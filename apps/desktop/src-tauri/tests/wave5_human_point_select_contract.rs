#![forbid(unsafe_code)]

#[test]
fn point_select_uses_stable_instrumentation_authority_and_not_selectors() {
    let instrumentation = include_str!("../../../../crates/instrumentation/src/lib.rs");

    for required in [
        "document.elementFromPoint",
        "refFor(freshTarget)",
        "validStableReference",
        "data-localview-owned",
        "pointer-events:none",
        "takePointSelectCompletions",
        "target_changed",
        "target_unavailable",
        "failPointSelectForRouteDrift",
        "data-localview-visual-freeze",
    ] {
        assert!(instrumentation.contains(required), "missing point-select authority: {required}");
    }

    let point_slice = instrumentation
        .split("const pointSelectCanonicalRoute")
        .nth(1)
        .expect("point-select slice");
    let point_slice = point_slice
        .split("const rectOf")
        .next()
        .expect("bounded point-select slice");
    for forbidden in ["querySelector(", "nth-child", "css selector", "DOM path", "innerHTML", "textContent:"] {
        assert!(!point_slice.contains(forbidden), "point selection must not invent selector/value authority: {forbidden}");
    }
}

#[test]
fn shell_point_selection_precedes_focus_without_rewriting_existing_actions() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let api = include_str!("../../src/api.ts");

    for required in [
        "pointSelectedReference ?? focusSelectedReference",
        "beginPointSelect",
        "pointSelectStatus",
        "cancelPointSelect",
        "pointSelectGeneration",
        "pointSelectTokenRef",
        "latestRouteSequence",
        "isStableElementReference(status.reference)",
        "selectedReference={selectedReference}",
        "onOpenSource={(reference) => void openSourceForSelection(reference)}",
        "onMeasure={(reference) => void measureCurrentSelection(reference)}",
        "onAskAi={(question) => void askAiAboutSelection(question)}",
    ] {
        assert!(shell.contains(required), "missing shell point-select wiring: {required}");
    }

    for required in [
        "'point_select_begin'",
        "'point_select_status'",
        "'point_select_cancel'",
    ] {
        assert!(api.contains(required), "missing API point-select command: {required}");
    }
}

#[test]
fn open_source_still_consumes_the_selected_stable_reference() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let source_request = [
        "const sourceOpenReference = reference;",
        "reference: sourceOpenReference,",
        "api.openSourceForSelection(sourceOpenRequest)",
        "sourceOpenReference !== selectedReferenceRef.current",
    ];

    for required in source_request {
        assert!(shell.contains(required), "existing source authority lost selected ref binding: {required}");
    }
    assert!(
        shell.contains("setPointSelectedReference(status.reference)"),
        "point selection must feed the same selectedReference authority used by Open Source"
    );
}

#[test]
fn bridge_receipt_is_bounded_and_generation_bound() {
    let desktop = include_str!("../src/lib.rs");
    let authority = include_str!("../src/point_select.rs");
    let permissions = include_str!("../permissions/localview.toml");

    for required in [
        "preview_complete_point_select",
        "requestToken",
        "bridgeGeneration: generation",
        "takePointSelectCompletions",
    ] {
        assert!(desktop.contains(required), "missing exact bridge binding: {required}");
    }

    for required in [
        "request_token",
        "session_id",
        "route",
        "bridge_generation",
        "valid_element_reference",
        "route_changed",
        "stale_request",
        "deny_unknown_fields",
    ] {
        assert!(authority.contains(required), "missing desktop race/privacy authority: {required}");
    }

    let transport_authority = authority
        .split("#[cfg(test)]")
        .next()
        .expect("production point-select authority");
    for forbidden in [
        "inner_html",
        "text_content",
        "input_value",
        "password",
        "cookies",
        "storage",
        "props",
        "hooks",
    ] {
        assert!(
            !transport_authority.contains(forbidden),
            "point-select receipt must not retain private payload: {forbidden}"
        );
    }

    assert!(permissions.contains(""point_select_begin""));
    assert!(permissions.contains(""preview_complete_point_select""));
}

#[test]
fn error_cancel_and_route_cleanup_are_explicit() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");
    let instrumentation = include_str!("../../../../crates/instrumentation/src/lib.rs");

    for required in [
        "pointSelectGeneration.current += 1",
        "pointSelectTokenRef.current = undefined",
        "setPointSelectActive(false)",
        "api.cancelPointSelect",
        "setPointSelectedReference(undefined)",
    ] {
        assert!(shell.contains(required), "missing shell cleanup: {required}");
    }

    for required in [
        "cleanupPointSelect",
        "removeEventListener(type, listener, true)",
        "freezeObserver?.disconnect()",
        "state.overlay?.remove()",
    ] {
        assert!(instrumentation.contains(required), "missing exact instrumentation cleanup: {required}");
    }
}
