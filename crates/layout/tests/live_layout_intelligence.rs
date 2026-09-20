use localview_layout::{
    DisplayMode, FlexDirection, FlexWrap, LayoutElement, LayoutIssueClass, LayoutStyleEvidence,
    OverflowMode, PositionMode, VisibilityEvidence, analyze, infer_spacing_scale,
};
use localview_protocol::Rect;

fn element(reference: &str, rect: Rect, parent: Option<&str>) -> LayoutElement {
    LayoutElement {
        reference: reference.into(),
        rect,
        parent: parent.map(str::to_owned),
        interactive: false,
        font_size: None,
        padding: None,
        style: LayoutStyleEvidence::default(),
        visibility: VisibilityEvidence::default(),
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn represents_horizontal_and_vertical_flex_without_reconstructing_css() {
    let mut horizontal = element("@row", rect(0.0, 0.0, 300.0, 80.0), None);
    horizontal.style.display = DisplayMode::Flex;
    horizontal.style.flex_direction = Some(FlexDirection::Row);
    horizontal.style.flex_wrap = Some(FlexWrap::Wrap);
    horizontal.style.justify_content = Some("space-between".into());
    horizontal.style.align_items = Some("center".into());
    horizontal.style.column_gap = Some(16.0);

    let mut vertical = element("@column", rect(0.0, 100.0, 180.0, 300.0), None);
    vertical.style.display = DisplayMode::InlineFlex;
    vertical.style.flex_direction = Some(FlexDirection::Column);
    vertical.style.row_gap = Some(12.0);

    let report = analyze(&[horizontal, vertical], (500.0, 500.0));
    let facts = report
        .facts
        .iter()
        .filter(|fact| fact.code == "flex_container")
        .collect::<Vec<_>>();
    assert_eq!(facts.len(), 2);
    assert!(facts.iter().any(|fact| fact.evidence.contains("Row")));
    assert!(facts.iter().any(|fact| fact.evidence.contains("Column")));
}

#[test]
fn represents_bounded_grid_tracks_as_observed_evidence() {
    let mut grid = element("@grid", rect(0.0, 0.0, 420.0, 300.0), None);
    grid.style.display = DisplayMode::Grid;
    grid.style.grid_template_columns = Some("120px 1fr 1fr".into());
    grid.style.grid_template_rows = Some("auto 1fr".into());
    grid.style.row_gap = Some(12.0);
    grid.style.column_gap = Some(16.0);

    let report = analyze(&[grid], (500.0, 500.0));
    let fact = report
        .facts
        .iter()
        .find(|fact| fact.code == "grid_container")
        .expect("grid fact");
    assert!(fact.evidence.contains("120px 1fr 1fr"));
    assert!(fact.evidence.contains("auto 1fr"));
}

#[test]
fn nested_containers_use_only_observed_parent_relationships() {
    let root = element("@root", rect(0.0, 0.0, 400.0, 400.0), None);
    let mut panel = element("@panel", rect(20.0, 20.0, 200.0, 200.0), Some("@root"));
    panel.style.overflow_x = Some(OverflowMode::Visible);
    panel.style.overflow_y = Some(OverflowMode::Visible);
    let child = element("@child", rect(190.0, 40.0, 80.0, 40.0), Some("@panel"));

    let report = analyze(&[root, panel, child], (400.0, 400.0));
    let issue = report
        .issues
        .iter()
        .find(|issue| issue.code == "container_overflow")
        .expect("child should exceed its actual parent");
    assert_eq!(
        issue.refs,
        vec!["@panel".to_string(), "@child".to_string()]
    );
    assert!(!issue.refs.contains(&"@root".to_string()));
}

#[test]
fn intentional_scroll_overflow_is_evidence_not_an_error() {
    let mut parent = element("@scroller", rect(0.0, 0.0, 120.0, 100.0), None);
    parent.style.overflow_x = Some(OverflowMode::Hidden);
    parent.style.overflow_y = Some(OverflowMode::Scroll);
    let child = element(
        "@long",
        rect(0.0, 0.0, 120.0, 320.0),
        Some("@scroller"),
    );

    let report = analyze(&[parent, child], (120.0, 100.0));
    assert!(
        report
            .facts
            .iter()
            .any(|fact| fact.code == "scroll_container_overflow")
    );
    assert!(!report.issues.iter().any(|issue| {
        issue.code == "container_overflow" || issue.code == "viewport_overflow"
    }));
}

#[test]
fn accidental_visible_container_overflow_is_reported() {
    let mut parent = element("@panel", rect(0.0, 0.0, 100.0, 100.0), None);
    parent.style.overflow_x = Some(OverflowMode::Visible);
    parent.style.overflow_y = Some(OverflowMode::Visible);
    let child = element("@child", rect(80.0, 10.0, 40.0, 40.0), Some("@panel"));

    let report = analyze(&[parent, child], (300.0, 300.0));
    let issue = report
        .issues
        .iter()
        .find(|issue| issue.code == "container_overflow")
        .expect("accidental overflow");
    assert_eq!(issue.class, LayoutIssueClass::Deterministic);
}

#[test]
fn clipped_child_is_not_mislabeled_as_accidental_overflow() {
    let mut parent = element("@clip", rect(0.0, 0.0, 100.0, 100.0), None);
    parent.style.overflow_x = Some(OverflowMode::Hidden);
    parent.style.overflow_y = Some(OverflowMode::Hidden);
    let mut child = element("@child", rect(70.0, 70.0, 80.0, 80.0), Some("@clip"));
    child.visibility.clipped = Some(true);

    let report = analyze(&[parent, child], (300.0, 300.0));
    assert!(
        report
            .facts
            .iter()
            .any(|fact| fact.code == "intentional_clip")
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code.contains("overflow"))
    );
}

#[test]
fn large_child_inside_large_container_is_valid() {
    let parent = element("@large", rect(0.0, 0.0, 900.0, 900.0), None);
    let child = element(
        "@child",
        rect(50.0, 50.0, 800.0, 800.0),
        Some("@large"),
    );

    let report = analyze(&[parent, child], (1000.0, 1000.0));
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code.contains("overflow"))
    );
}

#[test]
fn sibling_overlap_and_non_overlap_are_distinguished() {
    let parent = element("@parent", rect(0.0, 0.0, 500.0, 200.0), None);
    let left = element("@left", rect(10.0, 10.0, 100.0, 100.0), Some("@parent"));
    let overlapping = element(
        "@overlap",
        rect(55.0, 10.0, 100.0, 100.0),
        Some("@parent"),
    );
    let separate = element(
        "@separate",
        rect(300.0, 10.0, 100.0, 100.0),
        Some("@parent"),
    );

    let report = analyze(
        &[parent, left, overlapping, separate],
        (500.0, 300.0),
    );
    assert!(report.issues.iter().any(|issue| {
        issue.code == "sibling_collision"
            && issue.refs.contains(&"@left".to_string())
            && issue.refs.contains(&"@overlap".to_string())
    }));
    assert!(!report.issues.iter().any(|issue| {
        issue.code == "sibling_collision" && issue.refs.contains(&"@separate".to_string())
    }));
}

#[test]
fn fixed_or_sticky_collision_requires_sampled_occlusion_authority() {
    let mut blocker = element("@sticky", rect(0.0, 0.0, 300.0, 80.0), None);
    blocker.style.position = Some(PositionMode::Sticky);
    blocker.style.z_index = Some(10);

    let mut control = element("@button", rect(20.0, 20.0, 120.0, 40.0), None);
    control.interactive = true;
    control.visibility.sampled = true;
    control.visibility.occluded = Some(true);
    control.visibility.occluded_by = Some("@sticky".into());

    let report = analyze(&[blocker, control], (400.0, 300.0));
    let issue = report
        .issues
        .iter()
        .find(|issue| issue.code == "fixed_sticky_collision")
        .expect("supported fixed/sticky collision");
    assert_eq!(issue.class, LayoutIssueClass::Deterministic);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "control_occluded")
    );
}

#[test]
fn fixed_overlap_without_occlusion_authority_stays_unproven() {
    let mut blocker = element("@fixed", rect(0.0, 0.0, 300.0, 80.0), None);
    blocker.style.position = Some(PositionMode::Fixed);
    blocker.style.z_index = Some(999);
    let mut control = element("@button", rect(20.0, 20.0, 120.0, 40.0), None);
    control.interactive = true;

    let report = analyze(&[blocker, control], (400.0, 300.0));
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "fixed_sticky_collision")
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "control_occluded")
    );
}

#[test]
fn spacing_family_combines_recurring_gap_padding_and_edges_without_claiming_tokens() {
    let mut parent = element("@parent", rect(0.0, 0.0, 260.0, 100.0), None);
    parent.padding = Some([16.0, 16.0, 16.0, 16.0]);
    let a = element("@a", rect(16.0, 16.0, 40.0, 40.0), Some("@parent"));
    let b = element("@b", rect(72.0, 16.0, 40.0, 40.0), Some("@parent"));
    let c = element("@c", rect(128.2, 16.0, 40.0, 40.0), Some("@parent"));

    let report = analyze(&[parent, a, b, c], (300.0, 200.0));
    let family = report
        .spacing_families
        .iter()
        .find(|family| (family.value - 16.0).abs() < 0.5)
        .expect("recurring inferred spacing family");
    assert!(family.occurrences >= 4);

    let inferred = infer_spacing_scale(&[15.8, 16.0, 16.2, 23.9, 24.1]);
    assert_eq!(inferred.len(), 2);
    assert!((inferred[0] - 16.0).abs() < 0.3);
    assert!((inferred[1] - 24.0).abs() < 0.3);
}

#[test]
fn spacing_outlier_is_local_and_subpixel_jitter_is_not() {
    let parent = element("@parent", rect(0.0, 0.0, 400.0, 100.0), None);
    let a = element("@a", rect(0.0, 0.0, 40.0, 40.0), Some("@parent"));
    let b = element("@b", rect(56.0, 0.0, 40.0, 40.0), Some("@parent"));
    let c = element("@c", rect(112.3, 0.0, 40.0, 40.0), Some("@parent"));
    let d = element("@d", rect(182.3, 0.0, 40.0, 40.0), Some("@parent"));

    let report = analyze(&[parent, a, b, c, d], (500.0, 200.0));
    assert!(report.issues.iter().any(|issue| {
        issue.code == "spacing_outlier" && issue.evidence.contains("measured=30.000")
    }));
    assert!(!report.issues.iter().any(|issue| {
        issue.code == "spacing_outlier" && issue.evidence.contains("measured=16.300")
    }));
}

#[test]
fn alignment_families_are_parent_local_and_report_measured_deviation() {
    let parent = element("@parent", rect(0.0, 0.0, 300.0, 250.0), None);
    let a = element("@a", rect(20.0, 0.0, 60.0, 40.0), Some("@parent"));
    let b = element("@b", rect(20.4, 60.0, 72.0, 40.0), Some("@parent"));
    let outlier = element(
        "@outlier",
        rect(28.0, 120.0, 86.0, 40.0),
        Some("@parent"),
    );

    let report = analyze(&[parent, a, b, outlier], (400.0, 300.0));
    let issue = report
        .issues
        .iter()
        .find(|issue| {
            issue.code == "alignment_outlier" && issue.refs == vec!["@outlier".to_string()]
        })
        .expect("alignment outlier");
    assert!(issue.evidence.contains("expected_family=left_edge"));
    assert!(issue.evidence.contains("deviation="));
    assert!(issue.evidence.contains("threshold=2.500"));
}

#[test]
fn aligned_family_with_subpixel_jitter_does_not_emit_alignment_outlier() {
    let parent = element("@parent", rect(0.0, 0.0, 300.0, 250.0), None);
    let a = element("@a", rect(20.0, 0.0, 60.0, 40.0), Some("@parent"));
    let b = element("@b", rect(20.4, 60.0, 60.0, 40.0), Some("@parent"));
    let c = element("@c", rect(20.8, 120.0, 60.0, 40.0), Some("@parent"));

    let report = analyze(&[parent, a, b, c], (400.0, 300.0));
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.code == "alignment_outlier")
    );
}

#[test]
fn invalid_geometry_fails_closed_before_relational_checks() {
    let parent = element("@parent", rect(0.0, 0.0, 200.0, 200.0), None);
    let invalid = element(
        "@invalid",
        rect(10.0, 10.0, f64::NAN, 40.0),
        Some("@parent"),
    );
    let negative = element(
        "@negative",
        rect(10.0, 60.0, -4.0, 20.0),
        Some("@parent"),
    );
    let zero = element("@zero", rect(10.0, 100.0, 0.0, 20.0), Some("@parent"));

    let report = analyze(&[parent, invalid, negative, zero], (300.0, 300.0));
    assert!(report.issues.iter().any(|issue| {
        issue.code == "invalid_geometry" && issue.refs == vec!["@invalid".to_string()]
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.code == "invalid_geometry" && issue.refs == vec!["@negative".to_string()]
    }));
    assert!(report.issues.iter().any(|issue| {
        issue.code == "zero_area" && issue.refs == vec!["@zero".to_string()]
    }));
    assert!(!report.issues.iter().any(|issue| {
        issue.code == "sibling_collision" && issue.refs.contains(&"@invalid".to_string())
    }));
}

#[test]
fn node_retention_is_hard_bounded() {
    let elements = (0..600)
        .map(|index| {
            element(
                &format!("@node-{index}"),
                rect(
                    (index % 20) as f64 * 20.0,
                    (index / 20) as f64 * 20.0,
                    10.0,
                    10.0,
                ),
                None,
            )
        })
        .collect::<Vec<_>>();

    let report = analyze(&elements, (10_000.0, 10_000.0));
    assert_eq!(report.analyzed_nodes, 512);
    assert!(report.input_truncated);
}
