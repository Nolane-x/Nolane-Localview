#[test]
fn measure_is_a_read_only_bridge_action_and_consequential_actions_stay_closed() {
    let bridge = include_str!("../../../../crates/live-bridge/src/lib.rs");
    let control = include_str!("../../../../crates/control/src/runtime.rs");

    assert!(bridge.contains("Measure"));
    assert!(
        control.contains("BridgeActionKind::Snapshot | BridgeActionKind::Measure")
            || control.contains("BridgeActionKind::Measure | BridgeActionKind::Snapshot"),
        "simple public action queue must allow exactly Snapshot + Measure as read-only actions"
    );

    for action in ["Click", "TypeText", "Key", "Scroll", "Focus"] {
        assert!(
            control.contains(action),
            "control source must continue to contain consequential action marker {action}"
        );
    }
    assert!(control.contains("canonical_consequential_action_authority_required"));
}

#[test]
fn preview_measure_projects_geometry_only_from_localview_inspection() {
    let desktop = include_str!("../src/lib.rs");
    let execute = desktop
        .split("const execute = async (queued) =>")
        .nth(1)
        .expect("preview executor must exist")
        .split("const complete = async")
        .next()
        .expect("preview executor boundary must exist");

    assert!(execute.contains("case 'measure'"));
    assert!(execute.contains("api?.inspect?.(queued.reference)"));
    for marker in ["reference:", "rect:", "document_rect:", "viewport:", "route:"] {
        assert!(execute.contains(marker), "Measure projection is missing {marker}");
    }

    let measure_case = execute
        .split("case 'measure'")
        .nth(1)
        .expect("Measure case must exist")
        .split("default:")
        .next()
        .expect("Measure case must be bounded");
    for forbidden in ["sourceHint", "attributes", "ancestry", "style: inspected", "description"] {
        assert!(
            !measure_case.contains(forbidden),
            "Measure must not expose generic inspect field {forbidden}"
        );
    }
}

#[test]
fn desktop_measure_command_accepts_only_session_and_reference_authority() {
    let desktop = include_str!("../src/lib.rs");
    assert!(desktop.contains("async fn measure_current_selection("));
    let signature = desktop
        .split("async fn measure_current_selection(")
        .nth(1)
        .expect("Measure command must exist")
        .split(") -> Result<ElementMeasureReceipt, String>")
        .next()
        .expect("Measure command signature must be bounded");

    assert!(signature.contains("session_id: SessionId"));
    assert!(signature.contains("reference: String"));
    for forbidden in ["x:", "y:", "width:", "height:", "viewport:", "route:"] {
        assert!(
            !signature.contains(forbidden),
            "React must not author Measure geometry field {forbidden}"
        );
    }

    for marker in [
        "validate_measure_reference",
        "validate_measure_payload",
        "measure_current_selection",
        "MEASURE_RESULT_TIMEOUT",
    ] {
        assert!(desktop.contains(marker), "desktop Measure is missing {marker}");
    }
}

#[test]
fn frontend_measure_api_sends_reference_not_geometry() {
    let api = include_str!("../../src/api.ts");
    assert!(api.contains("measureElement"));
    assert!(api.contains("measure_current_selection"));

    let method = api
        .split("measureElement")
        .nth(1)
        .expect("Measure API must exist")
        .split("\n")
        .next()
        .expect("Measure API line must exist");
    assert!(method.contains("(sessionId: string, reference: string)"));
    assert!(method.contains("{ sessionId, reference }"));
    for forbidden in [" x:", " y:", " width:", " height:", " viewport:", " route:"] {
        assert!(!method.contains(forbidden));
    }
}

#[test]
fn inspector_measure_is_real_while_unwired_siblings_remain_fail_closed() {
    let tools = include_str!("../../src/features/FloatingTools.tsx");
    let inspector = tools
        .split("function Inspector(")
        .nth(1)
        .expect("Inspector must exist")
        .split("function AdvancedPanel(")
        .next()
        .expect("Inspector boundary must exist");

    assert!(inspector.contains("measureState"));
    assert!(inspector.contains("onMeasure"));
    assert!(inspector.contains("measure-action"));
    assert!(inspector.contains("RulerIcon"));
    assert!(
        !inspector.contains("UnavailableInspectorAction icon={<RulerIcon"),
        "Measure must leave the unavailable-action path"
    );

    for action in ["SourceIcon", "SparkIcon", "ActivityIcon"] {
        assert!(
            inspector.contains(action),
            "unwired sibling action must remain visible/fail-closed: {action}"
        );
    }
}

#[test]
fn shell_owns_measure_lifecycle_duplicate_guard_and_stale_selection_guard() {
    let shell = include_str!("../../src/app/LocalViewShell.tsx");

    for marker in [
        "HumanMeasureState",
        "measureState",
        "measureInFlight",
        "measureGeneration",
        "measureElement",
        "measureCurrentSelection",
        "selectedReference",
        "reference !== selectedReference",
        "onMeasure=",
    ] {
        assert!(
            shell.contains(marker),
            "Human-First Measure lifecycle is missing {marker}"
        );
    }
}

#[test]
fn measure_copy_is_localized_and_dictionary_complete() {
    let i18n = include_str!("../../src/i18n.ts");
    for key in [
        "measure.inProgress",
        "measure.success",
        "measure.failed",
        "measure.unavailable",
        "measure.selectFirst",
        "measure.position",
    ] {
        assert!(i18n.contains(&format!("'{key}'")), "missing Measure key {key}");
    }
    assert!(i18n.contains("type Dictionary = Record<MessageKey, string>;"));
}

#[test]
fn measure_runtime_audit_proves_authority_failure_and_races() {
    let capture = include_str!("../../../../tools/human-first-ui-v2/capture.mjs");

    for artifact in [
        "36-trusted-measure-ready.png",
        "37-trusted-measure-success.png",
        "38-trusted-measure-failure.png",
        "39-no-selection-measure-disabled.png",
        "40-trusted-measure-in-progress.png",
        "41-vi-trusted-measure-success.png",
        "42-trusted-measure-stale-selection.png",
    ] {
        assert!(
            capture.contains(artifact),
            "trusted Measure runtime audit is missing {artifact}"
        );
    }

    for marker in [
        "measure_current_selection",
        "no-caller-geometry",
        "measure-single-request",
        "measure-no-raw-error",
        "stale-measure-result-discarded",
    ] {
        assert!(
            capture.contains(marker),
            "trusted Measure runtime audit is missing invariant {marker}"
        );
    }
}

#[test]
fn main_dashboard_permission_allows_measure_command() {
    let permissions = include_str!("../permissions/localview.toml");
    let main = permissions
        .split("identifier = \"maincommands\"")
        .nth(1)
        .expect("main permission must exist")
        .split("[[permission]]")
        .next()
        .expect("main permission boundary must exist");

    assert!(main.contains("\"measure_current_selection\""));
}
