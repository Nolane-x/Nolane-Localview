use localview_engine::{choose_engine_authorized, EngineAdmissionError, EngineNeeds, EngineTier};
use localview_evidence::EvidenceKind;
use localview_planner::{
    plan_budgeted_perception_cycle, BudgetedPerceptionCandidate, PerceptionActionKind,
    PerceptionCandidate, PerceptionCycleSignals,
};
use localview_token_budget::{PerceptionBudgetContract, PerceptionBudgetUsage};

fn budget(chromium_spawns: u32) -> PerceptionBudgetContract {
    PerceptionBudgetContract {
        latency_ms: 1_500,
        text_tokens: 800,
        image_regions: 2,
        chromium_spawns,
    }
}

fn chromium_candidate() -> BudgetedPerceptionCandidate {
    BudgetedPerceptionCandidate {
        action: PerceptionCandidate {
            id: "chromium-compatibility-check".into(),
            kind: PerceptionActionKind::ChromiumEscalation,
            target: Some("save-button".into()),
            expected_evidence: vec![EvidenceKind::Visual],
            uncertainty_reduction: 1.0,
            risk_relevance: 1.0,
            estimated_cpu_ms: 20,
            estimated_tokens: 100,
            estimated_capture_bytes: 0,
        },
        estimated_usage: PerceptionBudgetUsage {
            latency_ms: 300,
            text_tokens: 100,
            image_regions: 0,
            chromium_spawns: 0,
        },
    }
}

fn chromium_needs() -> EngineNeeds {
    EngineNeeds {
        chrome_compatibility: true,
        ..Default::default()
    }
}

#[test]
fn tier_zero_to_two_keep_the_existing_engine_path_without_planner_ceremony() {
    let decision = choose_engine_authorized(
        &EngineNeeds {
            screenshot: true,
            ..Default::default()
        },
        None,
    )
    .expect("native WebView work does not require Tier-3 authorization");

    assert_eq!(decision.tier, EngineTier::NativeWebView);
}

#[test]
fn chromium_is_rejected_without_a_planner_selected_chromium_action() {
    let error = choose_engine_authorized(&chromium_needs(), None)
        .expect_err("Tier 3 must not be reachable without planner authority");

    assert_eq!(error, EngineAdmissionError::PlannerAuthorizationRequired);
}

#[test]
fn browser_specific_planner_selection_admits_exactly_one_chromium_tier() {
    let plan = plan_budgeted_perception_cycle(
        &[chromium_candidate()],
        &budget(0),
        &PerceptionCycleSignals {
            browser_specific_suspicion: true,
            ..Default::default()
        },
    );

    let decision = choose_engine_authorized(&chromium_needs(), Some(&plan))
        .expect("browser-specific planner authority should admit Tier 3");

    assert_eq!(decision.tier, EngineTier::Chromium);
    assert!(decision
        .reasons
        .iter()
        .any(|reason| reason.contains("planner-authorized")));
}

#[test]
fn a_non_chromium_planner_action_cannot_be_reused_to_unlock_tier_three() {
    let region = BudgetedPerceptionCandidate {
        action: PerceptionCandidate {
            id: "region".into(),
            kind: PerceptionActionKind::RegionCapture,
            target: Some("save-button".into()),
            expected_evidence: vec![EvidenceKind::Visual],
            uncertainty_reduction: 1.0,
            risk_relevance: 1.0,
            estimated_cpu_ms: 10,
            estimated_tokens: 100,
            estimated_capture_bytes: 16_384,
        },
        estimated_usage: PerceptionBudgetUsage {
            latency_ms: 100,
            text_tokens: 100,
            image_regions: 1,
            chromium_spawns: 0,
        },
    };
    let plan = plan_budgeted_perception_cycle(
        &[region],
        &budget(0),
        &PerceptionCycleSignals {
            explicit_deep_mode: true,
            ..Default::default()
        },
    );

    let error = choose_engine_authorized(&chromium_needs(), Some(&plan))
        .expect_err("a non-Chromium action must not authorize Tier 3");
    assert_eq!(error, EngineAdmissionError::PlannerAuthorizationRequired);
}

#[test]
fn chromium_budget_alone_does_not_bypass_browser_specific_planner_authority() {
    let plan = plan_budgeted_perception_cycle(
        &[chromium_candidate()],
        &budget(1),
        &PerceptionCycleSignals::default(),
    );
    assert!(plan.actions.is_empty());

    let error = choose_engine_authorized(&chromium_needs(), Some(&plan))
        .expect_err("budget capacity without browser suspicion is not authority");
    assert_eq!(error, EngineAdmissionError::PlannerAuthorizationRequired);
}
