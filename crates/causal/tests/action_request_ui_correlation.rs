use localview_causal::{
    correlate_action_request_ui, ActionCorrelationError, ActionCorrelationWindow,
    ActionRequestUiPolicy, CorrelationBasis, RuntimeSignal, RuntimeSignalKind,
};

fn signal(id: &str, kind: RuntimeSignalKind, observed_ms: u64, route: Option<&str>) -> RuntimeSignal {
    RuntimeSignal {
        id: id.into(),
        kind,
        observed_ms,
        route: route.map(str::to_owned),
        reference: None,
    }
}

#[test]
fn links_only_requests_followed_by_ui_response_inside_the_bounded_window() {
    let window = ActionCorrelationWindow {
        action_id: "action-1".into(),
        started_ms: 1_000,
        completed_ms: 1_200,
        route: Some("http://127.0.0.1:5173/cart".into()),
    };
    let signals = vec![
        signal("before", RuntimeSignalKind::Network, 900, Some("http://127.0.0.1:5173/cart")),
        signal("request", RuntimeSignalKind::Network, 1_050, Some("http://127.0.0.1:5173/cart")),
        signal("layout", RuntimeSignalKind::Layout, 1_100, Some("http://127.0.0.1:5173/cart")),
        signal("dom", RuntimeSignalKind::DomMutation, 1_130, Some("http://127.0.0.1:5173/cart")),
        signal("too-late", RuntimeSignalKind::Route, 3_000, Some("http://127.0.0.1:5173/cart")),
    ];

    let trace = correlate_action_request_ui(&window, &signals, &ActionRequestUiPolicy::default())
        .expect("valid bounded correlation");

    assert_eq!(trace.action_id, "action-1");
    assert_eq!(trace.links.len(), 1);
    assert_eq!(trace.links[0].request_id, "request");
    assert_eq!(trace.links[0].response_ids, vec!["layout", "dom"]);
    assert_eq!(trace.links[0].basis, CorrelationBasis::TemporalWindow);
    assert!(trace.links[0].confidence <= 0.65);
}

#[test]
fn exact_route_scope_excludes_background_events_from_other_routes() {
    let window = ActionCorrelationWindow {
        action_id: "action-2".into(),
        started_ms: 10,
        completed_ms: 20,
        route: Some("http://localhost:3000/a".into()),
    };
    let signals = vec![
        signal("wrong-request", RuntimeSignalKind::Network, 12, Some("http://localhost:3000/b")),
        signal("wrong-dom", RuntimeSignalKind::DomMutation, 13, Some("http://localhost:3000/b")),
        signal("right-request", RuntimeSignalKind::Network, 14, Some("http://localhost:3000/a")),
        signal("right-layout", RuntimeSignalKind::Layout, 15, Some("http://localhost:3000/a")),
    ];

    let trace = correlate_action_request_ui(&window, &signals, &ActionRequestUiPolicy::default())
        .expect("valid bounded correlation");

    assert_eq!(trace.links.len(), 1);
    assert_eq!(trace.links[0].request_id, "right-request");
    assert_eq!(trace.links[0].response_ids, vec!["right-layout"]);
}

#[test]
fn correlation_is_deterministic_and_bounded() {
    let window = ActionCorrelationWindow {
        action_id: "action-3".into(),
        started_ms: 0,
        completed_ms: 10,
        route: None,
    };
    let policy = ActionRequestUiPolicy {
        tail_ms: 100,
        max_signals: 3,
        max_responses_per_request: 1,
    };
    let signals = vec![
        signal("d", RuntimeSignalKind::Layout, 4, None),
        signal("b", RuntimeSignalKind::Network, 2, None),
        signal("c", RuntimeSignalKind::DomMutation, 3, None),
        signal("a", RuntimeSignalKind::Console, 1, None),
    ];

    let trace = correlate_action_request_ui(&window, &signals, &policy).expect("bounded trace");

    assert!(trace.truncated);
    assert_eq!(trace.observed_signal_count, 3);
    assert_eq!(trace.links.len(), 1);
    assert_eq!(trace.links[0].request_id, "b");
    assert_eq!(trace.links[0].response_ids, vec!["c"]);
    assert_eq!(trace.links[0].confidence, 0.55);
}

#[test]
fn invalid_windows_and_unbounded_policies_fail_closed() {
    let invalid_window = ActionCorrelationWindow {
        action_id: "action-4".into(),
        started_ms: 20,
        completed_ms: 10,
        route: None,
    };
    assert_eq!(
        correlate_action_request_ui(
            &invalid_window,
            &[],
            &ActionRequestUiPolicy::default()
        ),
        Err(ActionCorrelationError::InvalidWindow)
    );

    let valid_window = ActionCorrelationWindow {
        action_id: "action-5".into(),
        started_ms: 10,
        completed_ms: 20,
        route: None,
    };
    let invalid_policy = ActionRequestUiPolicy {
        tail_ms: 10_001,
        max_signals: 256,
        max_responses_per_request: 16,
    };
    assert_eq!(
        correlate_action_request_ui(&valid_window, &[], &invalid_policy),
        Err(ActionCorrelationError::InvalidPolicy)
    );
}


#[test]
fn requests_after_the_bounded_tail_are_excluded() {
    let window = ActionCorrelationWindow {
        action_id: "action-tail".into(),
        started_ms: 100,
        completed_ms: 200,
        route: Some("http://127.0.0.1:5173/".into()),
    };
    let policy = ActionRequestUiPolicy {
        tail_ms: 50,
        ..ActionRequestUiPolicy::default()
    };
    let signals = vec![
        signal(
            "inside-request",
            RuntimeSignalKind::Network,
            240,
            Some("http://127.0.0.1:5173/"),
        ),
        signal(
            "inside-layout",
            RuntimeSignalKind::Layout,
            245,
            Some("http://127.0.0.1:5173/"),
        ),
        signal(
            "late-request",
            RuntimeSignalKind::Network,
            251,
            Some("http://127.0.0.1:5173/"),
        ),
        signal(
            "late-layout",
            RuntimeSignalKind::Layout,
            252,
            Some("http://127.0.0.1:5173/"),
        ),
    ];

    let trace = correlate_action_request_ui(&window, &signals, &policy).expect("bounded trace");

    assert_eq!(trace.links.len(), 1);
    assert_eq!(trace.links[0].request_id, "inside-request");
    assert_eq!(trace.links[0].response_ids, vec!["inside-layout"]);
}

#[test]
fn response_count_is_capped_per_request() {
    let window = ActionCorrelationWindow {
        action_id: "action-response-cap".into(),
        started_ms: 0,
        completed_ms: 10,
        route: None,
    };
    let policy = ActionRequestUiPolicy {
        tail_ms: 100,
        max_signals: 16,
        max_responses_per_request: 2,
    };
    let signals = vec![
        signal("request", RuntimeSignalKind::Network, 1, None),
        signal("dom-1", RuntimeSignalKind::DomMutation, 2, None),
        signal("layout-1", RuntimeSignalKind::Layout, 3, None),
        signal("route-1", RuntimeSignalKind::Route, 4, None),
    ];

    let trace = correlate_action_request_ui(&window, &signals, &policy).expect("bounded trace");

    assert_eq!(trace.links.len(), 1);
    assert_eq!(trace.links[0].response_ids, vec!["dom-1", "layout-1"]);
}

#[test]
fn empty_action_identity_fails_closed() {
    let window = ActionCorrelationWindow {
        action_id: String::new(),
        started_ms: 10,
        completed_ms: 20,
        route: None,
    };

    assert_eq!(
        correlate_action_request_ui(&window, &[], &ActionRequestUiPolicy::default()),
        Err(ActionCorrelationError::InvalidWindow)
    );
}
