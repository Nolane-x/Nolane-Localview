use localview_engine::{choose_engine_authorized, EngineNeeds, EngineTier};
use localview_planner::{
    plan_budgeted_perception_cycle, BudgetedPerceptionCandidate, PerceptionActionKind,
    PerceptionCandidate, PerceptionCycleSignals,
};
use localview_token_budget::{PerceptionBudgetContract, PerceptionBudgetUsage};

fn rendered_candidate() -> BudgetedPerceptionCandidate {
    BudgetedPerceptionCandidate {
        action: PerceptionCandidate {
            id: "chromium-rendered".into(),
            kind: PerceptionActionKind::ChromiumRenderedCapture,
            target: Some("@save".into()),
            expected_evidence: Vec::new(),
            uncertainty_reduction: 1.0,
            risk_relevance: 1.0,
            estimated_cpu_ms: 20,
            estimated_tokens: 0,
            estimated_capture_bytes: 64 * 1024,
        },
        estimated_usage: PerceptionBudgetUsage {
            latency_ms: 500,
            text_tokens: 0,
            image_regions: 0,
            chromium_spawns: 0,
        },
    }
}

#[test]
fn planner_authorized_rendered_chromium_is_tier_three() {
    let plan = plan_budgeted_perception_cycle(
        &[rendered_candidate()],
        &PerceptionBudgetContract {
            latency_ms: 1_500,
            text_tokens: 800,
            image_regions: 2,
            chromium_spawns: 0,
        },
        &PerceptionCycleSignals {
            browser_specific_suspicion: true,
            ..Default::default()
        },
    );

    assert_eq!(plan.budget_decision.usage.chromium_spawns, 1);
    assert_eq!(plan.budget_decision.usage.image_regions, 1);

    let decision = choose_engine_authorized(
        &EngineNeeds {
            screenshot: true,
            chrome_compatibility: true,
            ..Default::default()
        },
        Some(&plan),
    )
    .expect("rendered Chromium must be admitted only through the exact planner action");

    assert_eq!(decision.tier, EngineTier::Chromium);
}
