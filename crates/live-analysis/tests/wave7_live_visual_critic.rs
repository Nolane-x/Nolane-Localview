use chrono::Utc;
use localview_design_grammar::MetricFamilies;
use localview_live_analysis::{analyze_live, analyze_visual_critic_events};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_quality::{FeatureState, SourceHintAuthority};
use serde_json::{Value, json};

fn snapshot(payload: Value) -> ObserverEvent {
    ObserverEvent {
        seq: 9,
        captured_at: Utc::now(),
        kind: ObserverEventKind::SemanticSnapshot,
        reference: None,
        route: Some("/fixture".into()),
        payload: json!({ "snapshot": payload }),
    }
}

fn live_payload() -> Value {
    json!({
        "version": 4,
        "viewport": { "width": 800.0, "height": 600.0 },
        "semantic_tree": {
            "ref": "root",
            "rect": { "x": 0.0, "y": 0.0, "width": 800.0, "height": 600.0 },
            "interactive": false,
            "style": {
                "fontSize": "16px",
                "fontWeight": "400",
                "lineHeight": "24px",
                "paddingTop": "16px",
                "paddingRight": "16px",
                "paddingBottom": "16px",
                "paddingLeft": "16px",
                "gap": "8px"
            },
            "children": [
                {
                    "ref": "save",
                    "rect": { "x": 16.0, "y": 16.0, "width": 120.0, "height": 40.0 },
                    "interactive": true,
                    "style": {
                        "fontSize": "16px",
                        "fontWeight": "400",
                        "lineHeight": "24px",
                        "paddingTop": "8px",
                        "paddingRight": "16px",
                        "paddingBottom": "8px",
                        "paddingLeft": "16px"
                    },
                    "sourceHint": { "file": "src/Button.tsx", "line": 12, "column": 3 },
                    "children": []
                },
                {
                    "ref": "cancel",
                    "rect": { "x": 144.0, "y": 16.0, "width": 120.0, "height": 40.0 },
                    "interactive": true,
                    "style": {
                        "fontSize": "16px",
                        "fontWeight": "400",
                        "lineHeight": "24px",
                        "paddingTop": "8px",
                        "paddingRight": "16px",
                        "paddingBottom": "8px",
                        "paddingLeft": "16px"
                    },
                    "children": []
                },
                {
                    "ref": "title",
                    "rect": { "x": 16.0, "y": 80.0, "width": 300.0, "height": 40.0 },
                    "interactive": false,
                    "style": {
                        "fontSize": "24px",
                        "fontWeight": "700",
                        "lineHeight": "32px",
                        "paddingTop": "0px",
                        "paddingRight": "0px",
                        "paddingBottom": "0px",
                        "paddingLeft": "0px"
                    },
                    "children": []
                },
                {
                    "ref": "title2",
                    "rect": { "x": 16.0, "y": 128.0, "width": 300.0, "height": 40.0 },
                    "interactive": false,
                    "style": {
                        "fontSize": "24px",
                        "fontWeight": "700",
                        "lineHeight": "32px",
                        "paddingTop": "0px",
                        "paddingRight": "0px",
                        "paddingBottom": "0px",
                        "paddingLeft": "0px"
                    },
                    "children": []
                }
            ]
        }
    })
}

#[test]
fn real_semantic_layout_evidence_flows_through_analyze_live() {
    let event = snapshot(live_payload());
    let result = analyze_live(&[event]);
    assert!(result.layout.analysis.analyzed_nodes >= 4);
    assert_eq!(result.visual_critic.snapshot_seq, Some(9));
    assert!(result.visual_critic.grammar.spacing_families.sample_count() > 0);
    assert!(
        result
            .visual_critic
            .grammar
            .type_size_families
            .families()
            .iter()
            .any(|family| (family.center - 16.0).abs() < 0.1)
    );
    assert!(
        result
            .visual_critic
            .grammar
            .control_height_families
            .families()
            .iter()
            .any(|family| (family.center - 40.0).abs() < 0.1)
    );
    assert!(matches!(
        result.visual_critic.critic.density,
        FeatureState::Available(_)
    ));
    assert!(result.visual_critic.baseline.is_some());
}

#[test]
fn unsupported_radius_evidence_is_unavailable_not_guessed() {
    let result = analyze_visual_critic_events(&[snapshot(live_payload())]);
    assert!(matches!(
        result.grammar.radius_families,
        MetricFamilies::Unavailable { .. }
    ));
}

#[test]
fn missing_viewport_is_inconclusive_not_fabricated() {
    let event = snapshot(json!({
        "version": 1,
        "semantic_tree": { "ref": "root", "children": [] }
    }));
    let result = analyze_visual_critic_events(&[event]);
    assert_eq!(
        result.availability_reason.as_deref(),
        Some("viewport_evidence_unavailable")
    );
    assert!(result.baseline.is_none());
}

#[test]
fn projection_does_not_retain_dom_text_attributes_or_values() {
    let mut payload = live_payload();
    payload["semantic_tree"]["name"] = json!("TOP SECRET USER TEXT");
    payload["semantic_tree"]["attributes"] = json!({
        "data-secret": "hunter2",
        "value": "personal input"
    });
    let result = analyze_visual_critic_events(&[snapshot(payload)]);
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("TOP SECRET"));
    assert!(!serialized.contains("hunter2"));
    assert!(!serialized.contains("personal input"));
    assert!(!serialized.contains("data-secret"));
}

#[test]
fn runtime_source_hint_never_masquerades_as_exact_css_authority() {
    let mut payload = live_payload();
    payload["semantic_tree"]["children"][2]["style"]["fontSize"] = json!("19px");
    payload["semantic_tree"]["children"][2]["sourceHint"] =
        json!({ "file": "src/Title.tsx", "line": 20, "column": 4 });
    let result = analyze_visual_critic_events(&[snapshot(payload)]);
    let hints = result
        .critic
        .findings
        .iter()
        .flat_map(|finding| finding.source_hints.iter())
        .collect::<Vec<_>>();
    assert!(!hints.is_empty());
    assert!(
        hints
            .iter()
            .all(|hint| hint.authority == SourceHintAuthority::ObservedRuntimeHint)
    );
}

#[test]
fn non_numeric_font_weight_and_non_px_line_height_are_unavailable_samples() {
    let mut payload = live_payload();
    for index in 0..=1 {
        payload["semantic_tree"]["children"][index]["style"]["fontWeight"] = json!("bold");
        payload["semantic_tree"]["children"][index]["style"]["lineHeight"] = json!("normal");
    }
    payload["semantic_tree"]["style"]["fontWeight"] = json!("normal");
    payload["semantic_tree"]["style"]["lineHeight"] = json!("normal");
    let result = analyze_visual_critic_events(&[snapshot(payload)]);
    assert!(
        result
            .grammar
            .font_weight_families
            .families()
            .iter()
            .all(|family| family.center != 400.0)
    );
}
