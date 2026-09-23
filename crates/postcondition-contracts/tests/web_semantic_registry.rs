use chrono::Utc;
use localview_postcondition_contracts::{
    PostconditionContractRegistry, RegisteredPostconditionContract,
    WebSemanticPostconditionContractV1, WebSemanticPostconditionEvaluation,
    WebSemanticPostconditionExpectation,
};
use localview_protocol::{PageSnapshot, Rect, SemanticNode};
use std::collections::BTreeMap;

fn node(reference: &str, name: &str, children: Vec<SemanticNode>) -> SemanticNode {
    SemanticNode {
        reference: reference.into(),
        role: Some("button".into()),
        name: Some(name.into()),
        tag: "button".into(),
        rect: Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 32.0,
        }),
        interactive: true,
        attributes: BTreeMap::new(),
        source: None,
        ownership: None,
        children,
    }
}

fn snapshot(children: Vec<SemanticNode>) -> PageSnapshot {
    PageSnapshot {
        version: 9,
        route: "/settings".into(),
        viewport: (1280, 720),
        root: SemanticNode {
            reference: "@e1".into(),
            role: Some("main".into()),
            name: Some("Settings".into()),
            tag: "main".into(),
            rect: None,
            interactive: false,
            attributes: BTreeMap::new(),
            source: None,
            ownership: None,
            children,
        },
        console_errors: vec![],
        failed_requests: vec![],
        captured_at: Utc::now(),
    }
}

#[test]
fn web_semantic_v1_is_canonical_registered_and_exact_ref_bound() {
    let contract = WebSemanticPostconditionContractV1 {
        expectation: WebSemanticPostconditionExpectation::Present,
        reference: "@edead".into(),
    };
    let encoded = contract.to_contract_ref().unwrap();
    assert_eq!(
        encoded,
        r#"lvpc:web-semantic:v1:{"expectation":"present","ref":"@edead"}"#
    );

    let registry = PostconditionContractRegistry::standard();
    assert_eq!(
        registry.decode(&encoded).unwrap(),
        RegisteredPostconditionContract::WebSemanticV1(contract.clone())
    );
    assert_eq!(
        registry
            .evaluate_web_semantic(&encoded, &snapshot(vec![node("@edead", "Done", vec![])]))
            .unwrap(),
        WebSemanticPostconditionEvaluation::VerifiedPass
    );
    assert_eq!(
        registry
            .evaluate_web_semantic(&encoded, &snapshot(vec![node("@ebeef", "Other", vec![])]))
            .unwrap(),
        WebSemanticPostconditionEvaluation::VerifiedFail
    );
}

#[test]
fn web_semantic_absence_is_proved_only_against_the_fresh_snapshot_supplied() {
    let contract = WebSemanticPostconditionContractV1 {
        expectation: WebSemanticPostconditionExpectation::Absent,
        reference: "@ebad".into(),
    };
    let encoded = contract.to_contract_ref().unwrap();
    let registry = PostconditionContractRegistry::standard();

    assert_eq!(
        registry
            .evaluate_web_semantic(&encoded, &snapshot(vec![node("@ecafe", "Ready", vec![])]))
            .unwrap(),
        WebSemanticPostconditionEvaluation::VerifiedPass
    );
    assert_eq!(
        registry
            .evaluate_web_semantic(&encoded, &snapshot(vec![node("@ebad", "Error", vec![])]))
            .unwrap(),
        WebSemanticPostconditionEvaluation::VerifiedFail
    );
}
