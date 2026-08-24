use std::collections::BTreeMap;

use chrono::Utc;
use localview_capture::{
    resolve_progressive_targets, ProgressiveTargetError, ProgressiveTargetKind,
    ProgressiveTargetProvenance,
};
use localview_protocol::{PageSnapshot, Rect, SemanticNode, SourceLocation};

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn node(
    reference: &str,
    tag: &str,
    role: Option<&str>,
    bounds: Option<Rect>,
    component: Option<&str>,
    children: Vec<SemanticNode>,
) -> SemanticNode {
    SemanticNode {
        reference: reference.into(),
        role: role.map(str::to_owned),
        name: None,
        tag: tag.into(),
        rect: bounds,
        interactive: false,
        attributes: BTreeMap::new(),
        source: component.map(|name| SourceLocation {
            file: format!("{name}.tsx"),
            line: 1,
            column: Some(1),
            component: Some(name.into()),
        }),
        children,
    }
}

fn snapshot(root: SemanticNode) -> PageSnapshot {
    PageSnapshot {
        version: 7,
        route: "/settings".into(),
        viewport: (1000, 800),
        root,
        console_errors: vec![],
        failed_requests: vec![],
        captured_at: Utc::now(),
    }
}

fn target_kinds(plan: &localview_capture::ProgressiveTargetPlan) -> Vec<ProgressiveTargetKind> {
    plan.targets.iter().map(|target| target.kind).collect()
}

#[test]
fn resolves_expanded_element_component_section_and_viewport_in_order() {
    let target = node(
        "@save",
        "button",
        Some("button"),
        Some(rect(300.0, 300.0, 100.0, 40.0)),
        Some("SettingsCard"),
        vec![],
    );
    let component = node(
        "@card",
        "div",
        None,
        Some(rect(250.0, 220.0, 500.0, 300.0)),
        Some("SettingsCard"),
        vec![target],
    );
    let section = node(
        "@section",
        "section",
        Some("region"),
        Some(rect(200.0, 180.0, 650.0, 420.0)),
        None,
        vec![component],
    );
    let root = node(
        "@root",
        "main",
        Some("main"),
        Some(rect(0.0, 0.0, 1000.0, 800.0)),
        None,
        vec![section],
    );

    let plan = resolve_progressive_targets(&snapshot(root), "@save").expect("resolve targets");

    assert_eq!(plan.reference, "@save");
    assert_eq!(plan.snapshot_version, 7);
    assert_eq!(plan.route, "/settings");
    assert_eq!(plan.viewport, (1000, 800));
    assert_eq!(
        target_kinds(&plan),
        vec![
            ProgressiveTargetKind::Element,
            ProgressiveTargetKind::Component,
            ProgressiveTargetKind::Section,
            ProgressiveTargetKind::Viewport,
        ]
    );
    assert_eq!(plan.targets[0].rect, rect(180.0, 180.0, 340.0, 280.0));
    assert_eq!(plan.targets[0].confidence_milli, 1000);
    assert!(matches!(
        &plan.targets[0].provenance,
        ProgressiveTargetProvenance::StableElementRef { reference } if reference == "@save"
    ));
    assert_eq!(plan.targets[1].rect, rect(250.0, 220.0, 500.0, 300.0));
    assert!(matches!(
        &plan.targets[1].provenance,
        ProgressiveTargetProvenance::SourceComponent { component, owner_ref }
            if component == "SettingsCard" && owner_ref == "@card"
    ));
    assert_eq!(plan.targets[2].rect, rect(200.0, 180.0, 650.0, 420.0));
    assert!(matches!(
        &plan.targets[2].provenance,
        ProgressiveTargetProvenance::SemanticSection { owner_ref, .. } if owner_ref == "@section"
    ));
    assert_eq!(plan.targets[3].rect, rect(0.0, 0.0, 1000.0, 800.0));
}

#[test]
fn component_is_not_fabricated_without_matching_source_ancestor() {
    let target = node(
        "@save",
        "button",
        Some("button"),
        Some(rect(300.0, 300.0, 100.0, 40.0)),
        Some("SettingsCard"),
        vec![],
    );
    let mismatched_parent = node(
        "@card",
        "div",
        None,
        Some(rect(250.0, 220.0, 500.0, 300.0)),
        Some("OtherCard"),
        vec![target],
    );
    let root = node(
        "@root",
        "main",
        Some("main"),
        Some(rect(0.0, 0.0, 1000.0, 800.0)),
        None,
        vec![mismatched_parent],
    );

    let plan = resolve_progressive_targets(&snapshot(root), "@save").expect("resolve targets");
    assert_eq!(
        target_kinds(&plan),
        vec![ProgressiveTargetKind::Element, ProgressiveTargetKind::Section, ProgressiveTargetKind::Viewport]
    );
}

#[test]
fn nearest_explicit_section_or_landmark_ancestor_wins() {
    let target = node(
        "@field",
        "input",
        None,
        Some(rect(50.0, 50.0, 120.0, 30.0)),
        None,
        vec![],
    );
    let nearest = node(
        "@form",
        "div",
        Some("form"),
        Some(rect(20.0, 20.0, 300.0, 160.0)),
        None,
        vec![target],
    );
    let outer = node(
        "@article",
        "article",
        None,
        Some(rect(0.0, 0.0, 600.0, 500.0)),
        None,
        vec![nearest],
    );
    let root = node(
        "@root",
        "div",
        None,
        Some(rect(0.0, 0.0, 1000.0, 800.0)),
        None,
        vec![outer],
    );

    let plan = resolve_progressive_targets(&snapshot(root), "@field").expect("resolve targets");
    let section = plan
        .targets
        .iter()
        .find(|target| target.kind == ProgressiveTargetKind::Section)
        .expect("section target");
    assert_eq!(section.rect, rect(20.0, 20.0, 300.0, 160.0));
    assert!(matches!(
        &section.provenance,
        ProgressiveTargetProvenance::SemanticSection { owner_ref, .. } if owner_ref == "@form"
    ));
}

#[test]
fn duplicate_component_or_section_rect_is_removed_without_reordering() {
    let shared = rect(100.0, 100.0, 500.0, 300.0);
    let target = node(
        "@target",
        "button",
        None,
        Some(rect(250.0, 200.0, 100.0, 40.0)),
        Some("Card"),
        vec![],
    );
    let component = node("@component", "div", None, Some(shared.clone()), Some("Card"), vec![target]);
    let section = node("@section", "section", Some("region"), Some(shared), None, vec![component]);
    let root = node("@root", "div", None, Some(rect(0.0, 0.0, 1000.0, 800.0)), None, vec![section]);

    let plan = resolve_progressive_targets(&snapshot(root), "@target").expect("resolve targets");
    assert_eq!(
        target_kinds(&plan),
        vec![ProgressiveTargetKind::Element, ProgressiveTargetKind::Component, ProgressiveTargetKind::Viewport]
    );
}

#[test]
fn missing_ref_or_invalid_target_geometry_fails_closed() {
    let valid_root = node(
        "@root",
        "main",
        Some("main"),
        Some(rect(0.0, 0.0, 1000.0, 800.0)),
        None,
        vec![],
    );
    let missing = resolve_progressive_targets(&snapshot(valid_root), "@missing")
        .expect_err("missing stable ref must fail");
    assert!(matches!(missing, ProgressiveTargetError::ReferenceNotFound));

    let invalid = node(
        "@bad",
        "button",
        None,
        Some(rect(f64::NAN, 10.0, 100.0, 40.0)),
        None,
        vec![],
    );
    let invalid_root = node(
        "@root",
        "main",
        Some("main"),
        Some(rect(0.0, 0.0, 1000.0, 800.0)),
        None,
        vec![invalid],
    );
    let error = resolve_progressive_targets(&snapshot(invalid_root), "@bad")
        .expect_err("non-finite geometry must fail");
    assert!(matches!(error, ProgressiveTargetError::InvalidElementGeometry));

    let offscreen = node(
        "@offscreen",
        "button",
        None,
        Some(rect(1200.0, 900.0, 100.0, 40.0)),
        None,
        vec![],
    );
    let offscreen_root = node(
        "@root",
        "main",
        Some("main"),
        Some(rect(0.0, 0.0, 1000.0, 800.0)),
        None,
        vec![offscreen],
    );
    let error = resolve_progressive_targets(&snapshot(offscreen_root), "@offscreen")
        .expect_err("fully offscreen geometry must fail");
    assert!(matches!(error, ProgressiveTargetError::InvalidElementGeometry));
}
