use localview_evidence::EvidenceKind;
use localview_planner::{
    plan_budgeted_perception_cycle, BudgetedPerceptionCandidate, PerceptionActionKind,
    PerceptionCandidate, PerceptionCycleSignals, PerceptionPlanRejectionReason,
};
use localview_token_budget::{
    BudgetEscalationReason, PerceptionBudgetContract, PerceptionBudgetUsage,
};

fn candidate(kind: PerceptionActionKind) -> BudgetedPerceptionCandidate {
    BudgetedPerceptionCandidate {
        action: PerceptionCandidate {
            id: "chromium-rendered".into(),
            kind,
            target: Some("@save".into()),
            expected_evidence: vec![EvidenceKind::Visual],
            uncertainty_reduction: 1.0,
            risk_relevance: 1.0,
            estimated_cpu_ms: 20,
            estimated_tokens: 100,
            estimated_capture_bytes: 64 * 1024,
        },
        // Authority must normalize these two zeroes rather than trusting the candidate.
        estimated_usage: PerceptionBudgetUsage {
            latency_ms: 500,
            text_tokens: 0,
            image_regions: 0,
            chromium_spawns: 0,
        },
    }
}

fn budget() -> PerceptionBudgetContract {
    PerceptionBudgetContract {
        latency_ms: 1_500,
        text_tokens: 800,
        image_regions: 2,
        chromium_spawns: 0,
    }
}

#[test]
fn rendered_chromium_forces_exactly_one_spawn_and_one_image_region() {
    let plan = plan_budgeted_perception_cycle(
        &[candidate(PerceptionActionKind::ChromiumRenderedCapture)],
        &budget(),
        &PerceptionCycleSignals {
            browser_specific_suspicion: true,
            ..Default::default()
        },
    );

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(
        plan.actions[0].action.kind,
        PerceptionActionKind::ChromiumRenderedCapture
    );
    assert_eq!(plan.budget_decision.usage.chromium_spawns, 1);
    assert_eq!(plan.budget_decision.usage.image_regions, 1);
    assert_eq!(
        plan.budget_decision.budget_escalation_reason,
        Some(BudgetEscalationReason::BrowserSpecificSuspicion)
    );
}

#[test]
fn rendered_chromium_is_rejected_without_browser_specific_suspicion() {
    let plan = plan_budgeted_perception_cycle(
        &[candidate(PerceptionActionKind::ChromiumRenderedCapture)],
        &budget(),
        &PerceptionCycleSignals {
            explicit_deep_mode: true,
            ..Default::default()
        },
    );

    assert!(plan.actions.is_empty());
    assert_eq!(
        plan.rejected[0].reason,
        PerceptionPlanRejectionReason::ChromiumRequiresBrowserSpecificSuspicion
    );
}
