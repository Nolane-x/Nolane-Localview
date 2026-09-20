use chrono::Utc;
use localview_layout::LayoutIssueClass;
use localview_live_analysis::{analyze_live, diagnose_live};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use serde_json::json;

fn snapshot_event(seq: u64, snapshot: serde_json::Value) -> ObserverEvent {
    ObserverEvent {
        seq,
        captured_at: Utc::now(),
        kind: ObserverEventKind::SemanticSnapshot,
        reference: None,
        route: Some("/layout".into()),
        payload: json!({"type":"semantic_snapshot","snapshot":snapshot}),
    }
}

#[test]
fn live_semantic_snapshot_is_projected_into_layout_analyzer() {
    let report = analyze_live(&[snapshot_event(
        7,
        json!({
            "version": 11,
            "viewport": {"width": 400, "height": 300, "dpr": 1},
            "semantic_tree": {
                "ref": "@root",
                "rect": {"x":0.0,"y":0.0,"width":400.0,"height":300.0},
                "interactive": false,
                "visibility": {"inViewport":true,"clipped":false,"occluded":false,"occludedBy":null,"sampled":true},
                "style": {"display":"block","position":"static","overflowX":"visible","overflowY":"visible"},
                "children": [{
                    "ref": "@row",
                    "rect": {"x":20.0,"y":20.0,"width":180.0,"height":80.0},
                    "interactive": false,
                    "visibility": {"inViewport":true,"clipped":false,"occluded":false,"occludedBy":null,"sampled":true},
                    "style": {
                        "display":"flex","position":"static","overflowX":"visible","overflowY":"visible",
                        "flexDirection":"row","flexWrap":"nowrap","justifyContent":"space-between","alignItems":"center",
                        "gap":"16px","rowGap":"16px","columnGap":"16px",
                        "paddingTop":"16px","paddingRight":"16px","paddingBottom":"16px","paddingLeft":"16px"
                    },
                    "children": [{
                        "ref": "@button",
                        "rect": {"x":170.0,"y":30.0,"width":60.0,"height":40.0},
                        "interactive": true,
                        "visibility": {"inViewport":true,"clipped":false,"occluded":false,"occludedBy":null,"sampled":true},
                        "style": {"display":"block","position":"static","overflowX":"visible","overflowY":"visible"},
                        "children": []
                    }]
                }]
            }
        }),
    )]);

    assert_eq!(report.layout.snapshot_seq, Some(7));
    assert_eq!(report.layout.snapshot_version, Some(11));
    assert_eq!(report.layout.analysis.analyzed_nodes, 3);
    assert!(
        report
            .layout
            .analysis
            .facts
            .iter()
            .any(|fact| fact.code == "flex_container" && fact.refs == vec!["@row".to_string()])
    );
    let overflow = report
        .layout
        .analysis
        .issues
        .iter()
        .find(|issue| issue.code == "container_overflow")
        .expect("live geometry and bounded overflow style should reach analyzer");
    assert_eq!(overflow.class, LayoutIssueClass::Deterministic);
    assert_eq!(
        overflow.refs,
        vec!["@row".to_string(), "@button".to_string()]
    );
}

#[test]
fn latest_semantic_snapshot_wins_and_raw_retained_snapshot_shape_is_supported() {
    let older = snapshot_event(
        2,
        json!({
            "version": 2,
            "viewport":{"width":100,"height":100},
            "semantic_tree":{"ref":"@old","rect":{"x":0.0,"y":0.0,"width":100.0,"height":100.0},"children":[]}
        }),
    );
    let newer = ObserverEvent {
        seq: 9,
        captured_at: Utc::now(),
        kind: ObserverEventKind::SemanticSnapshot,
        reference: None,
        route: Some("/layout".into()),
        payload: json!({
            "version": 9,
            "viewport":{"width":200,"height":120},
            "semantic_tree":{"ref":"@new","rect":{"x":0.0,"y":0.0,"width":200.0,"height":120.0},"children":[]}
        }),
    };

    let report = analyze_live(&[newer, older]);
    assert_eq!(report.layout.snapshot_seq, Some(9));
    assert_eq!(report.layout.snapshot_version, Some(9));
    assert_eq!(report.layout.analysis.analyzed_nodes, 1);
}

#[test]
fn arbitrary_style_fields_and_private_text_are_not_retained_in_layout_output() {
    let report = analyze_live(&[snapshot_event(
        3,
        json!({
            "version": 3,
            "viewport":{"width":200,"height":120},
            "semantic_tree":{
                "ref":"@root",
                "name":"secret visible text",
                "rect":{"x":0.0,"y":0.0,"width":200.0,"height":120.0},
                "style":{
                    "display":"grid",
                    "gridTemplateColumns":"1fr 1fr",
                    "backgroundImage":"url(https://example.test/private-token)",
                    "customSecret":"should-not-flow"
                },
                "children":[]
            }
        }),
    )]);

    let serialized = serde_json::to_string(&report.layout).expect("serialize layout report");
    assert!(!serialized.contains("secret visible text"));
    assert!(!serialized.contains("backgroundImage"));
    assert!(!serialized.contains("private-token"));
    assert!(!serialized.contains("customSecret"));
    assert!(serialized.contains("grid_container"));
}


#[test]
fn missing_or_invalid_live_viewport_is_unknown_not_a_critical_layout_finding() {
    for snapshot in [
        json!({"version": 21}),
        json!({
            "version": 22,
            "viewport": {"width": 0, "height": 720},
            "semantic_tree": {
                "ref": "@root",
                "rect": {"x":0.0,"y":0.0,"width":400.0,"height":300.0},
                "children": []
            }
        }),
    ] {
        let event = snapshot_event(21, snapshot);
        let analysis = analyze_live(std::slice::from_ref(&event));
        assert_eq!(analysis.layout.snapshot_seq, Some(21));
        assert_eq!(analysis.layout.analysis.analyzed_nodes, 0);
        assert!(analysis.layout.analysis.issues.is_empty());

        let diagnosis = diagnose_live(&[event]);
        assert!(
            !diagnosis
                .findings
                .iter()
                .any(|finding| finding.code == "invalid_viewport_geometry"),
            "unverified live viewport must not become an application defect"
        );
        assert!(
            diagnosis
                .unknowns
                .iter()
                .any(|unknown| unknown.statement == "Current layout geometry has not been verified")
        );
    }
}
