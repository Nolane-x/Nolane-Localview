use localview_capture::{
    build_plan, evaluate_settle, CaptureStage, CaptureTarget, SettleObservation, SettleReason,
    StableCapturePolicy,
};
use serde_json::json;

fn ready() -> SettleObservation {
    serde_json::from_value(json!({
        "now_unix_ms": 10_000,
        "latest_semantic_at_unix_ms": 9_900,
        "ready_state": "complete",
        "fonts_status": "loaded",
        "pending_images": 0,
        "inflight_network_requests": 0,
        "latest_hmr_at_unix_ms": 9_000,
        "latest_dom_mutation_at_unix_ms": 9_000,
        "latest_layout_at_unix_ms": 9_000,
        "latest_network_at_unix_ms": 9_000
    }))
    .expect("ready observation")
}

fn reason_names(observation: &SettleObservation, policy: &StableCapturePolicy) -> Vec<String> {
    serde_json::to_value(evaluate_settle(policy, observation).reasons)
        .expect("reasons serialize")
        .as_array()
        .expect("reason array")
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn fully_ready_and_quiet_page_is_stable() {
    let decision = evaluate_settle(&StableCapturePolicy::default(), &ready());
    assert!(decision.stable);
    assert!(decision.reasons.is_empty());
    assert!((25..=100).contains(&decision.retry_after_ms));
}

#[test]
fn active_network_requests_block_even_when_last_completion_is_old() {
    let observation: SettleObservation = serde_json::from_value(json!({
        "now_unix_ms": 10_000,
        "latest_semantic_at_unix_ms": 9_900,
        "ready_state": "complete",
        "fonts_status": "loaded",
        "pending_images": 0,
        "inflight_network_requests": 2,
        "latest_hmr_at_unix_ms": 9_000,
        "latest_dom_mutation_at_unix_ms": 9_000,
        "latest_layout_at_unix_ms": 9_000,
        "latest_network_at_unix_ms": 9_000
    }))
    .expect("observation");

    let decision = evaluate_settle(&StableCapturePolicy::default(), &observation);
    let reasons = reason_names(&observation, &StableCapturePolicy::default());
    assert!(!decision.stable);
    assert!(reasons.iter().any(|reason| reason == "network_inflight"));
}

#[test]
fn missing_network_counter_fails_closed_when_network_gate_is_enabled() {
    let observation: SettleObservation = serde_json::from_value(json!({
        "now_unix_ms": 10_000,
        "latest_semantic_at_unix_ms": 9_900,
        "ready_state": "complete",
        "fonts_status": "loaded",
        "pending_images": 0,
        "latest_hmr_at_unix_ms": 9_000,
        "latest_dom_mutation_at_unix_ms": 9_000,
        "latest_layout_at_unix_ms": 9_000,
        "latest_network_at_unix_ms": 9_000
    }))
    .expect("observation");

    let decision = evaluate_settle(&StableCapturePolicy::default(), &observation);
    let reasons = reason_names(&observation, &StableCapturePolicy::default());
    assert!(!decision.stable);
    assert!(reasons.iter().any(|reason| reason == "network_state_unknown"));
}

#[test]
fn disabled_network_gate_does_not_require_inflight_counter() {
    let observation: SettleObservation = serde_json::from_value(json!({
        "now_unix_ms": 10_000,
        "latest_semantic_at_unix_ms": 9_900,
        "ready_state": "complete",
        "fonts_status": "loaded",
        "pending_images": 0,
        "latest_hmr_at_unix_ms": 9_000,
        "latest_dom_mutation_at_unix_ms": 9_000,
        "latest_layout_at_unix_ms": 9_000,
        "latest_network_at_unix_ms": 9_999
    }))
    .expect("observation");
    let policy = StableCapturePolicy {
        network_quiet_ms: None,
        ..StableCapturePolicy::default()
    };

    let decision = evaluate_settle(&policy, &observation);
    assert!(decision.stable, "unexpected reasons: {:?}", decision.reasons);
}

#[test]
fn missing_snapshot_blocks_required_readiness() {
    let mut observation = ready();
    observation.latest_semantic_at_unix_ms = None;
    observation.ready_state = None;
    observation.fonts_status = None;
    observation.pending_images = None;
    let decision = evaluate_settle(&StableCapturePolicy::default(), &observation);
    assert!(!decision.stable);
    assert!(decision.reasons.contains(&SettleReason::NoSemanticSnapshot));
}

#[test]
fn readiness_and_recent_runtime_activity_report_independent_reasons() {
    let mut observation = ready();
    observation.ready_state = Some("interactive".into());
    observation.fonts_status = Some("loading".into());
    observation.pending_images = Some(2);
    observation.latest_hmr_at_unix_ms = Some(9_800);
    observation.latest_dom_mutation_at_unix_ms = Some(9_850);
    observation.latest_layout_at_unix_ms = Some(9_850);
    observation.latest_network_at_unix_ms = Some(9_900);

    let decision = evaluate_settle(&StableCapturePolicy::default(), &observation);
    for reason in [
        SettleReason::DomNotReady,
        SettleReason::FontsPending,
        SettleReason::ImagesPending,
        SettleReason::HmrRecent,
        SettleReason::DomMutationRecent,
        SettleReason::LayoutRecent,
        SettleReason::NetworkRecent,
    ] {
        assert!(decision.reasons.contains(&reason), "missing {reason:?}");
    }
}

#[test]
fn events_at_or_outside_quiet_windows_do_not_block() {
    let mut observation = ready();
    observation.latest_hmr_at_unix_ms = Some(9_700);
    observation.latest_dom_mutation_at_unix_ms = Some(9_800);
    observation.latest_layout_at_unix_ms = Some(9_800);
    observation.latest_network_at_unix_ms = Some(9_750);
    let decision = evaluate_settle(&StableCapturePolicy::default(), &observation);
    assert!(decision.stable, "unexpected reasons: {:?}", decision.reasons);
}

#[test]
fn disabled_policy_gate_removes_only_its_reason() {
    let policy = StableCapturePolicy {
        wait_fonts: false,
        ..StableCapturePolicy::default()
    };
    let mut observation = ready();
    observation.fonts_status = Some("loading".into());
    observation.pending_images = Some(1);

    let decision = evaluate_settle(&policy, &observation);
    assert!(!decision.reasons.contains(&SettleReason::FontsPending));
    assert!(decision.reasons.contains(&SettleReason::ImagesPending));
}

#[test]
fn future_event_timestamp_is_treated_as_recent() {
    let mut observation = ready();
    observation.latest_layout_at_unix_ms = Some(10_100);
    let decision = evaluate_settle(&StableCapturePolicy::default(), &observation);
    assert!(decision.reasons.contains(&SettleReason::LayoutRecent));
}

#[test]
fn default_capture_policy_masks_common_private_and_credential_surfaces() {
    let policy = StableCapturePolicy::default();
    let expected = [
        "[data-localview-private]",
        "[data-private]",
        "[data-sensitive]",
        "input[type=\"password\"]",
        "input[autocomplete=\"current-password\"]",
        "input[autocomplete=\"new-password\"]",
        "input[autocomplete=\"one-time-code\"]",
    ];

    for selector in expected {
        assert!(
            policy.mask_selectors.iter().any(|candidate| candidate == selector),
            "missing default private selector: {selector}"
        );
    }
    assert!(policy.mask_selectors.len() <= 16);

    let plan = build_plan(CaptureTarget::Viewport, policy);
    assert!(plan.stages.contains(&CaptureStage::Masked));
}
