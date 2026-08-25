use localview_evidence::EvidenceKind;
use localview_planner::{
    plan_budgeted_perception_cycle, BudgetedPerceptionCandidate, PerceptionActionKind,
    PerceptionCandidate, PerceptionCycleSignals, PerceptionPlanRejectionReason,
};
use localview_token_budget::{
    BudgetEscalationReason, PerceptionBudgetContract, PerceptionBudgetDecisionStatus,
    PerceptionBudgetUsage,
};

fn budget() -> PerceptionBudgetContract {
    PerceptionBudgetContract {
        latency_ms: 1_500,
        text_tokens: 800,
        image_regions: 2,
        chromium_spawns: 0,
    }
}

fn candidate(
    id: &str,
    kind: PerceptionActionKind,
    uncertainty_reduction: f32,
    usage: PerceptionBudgetUsage,
) -> BudgetedPerceptionCandidate {
    BudgetedPerceptionCandidate {
        action: PerceptionCandidate {
            id: id.into(),
            kind,
            target: Some("save-button".into()),
            expected_evidence: vec![EvidenceKind::Visual],
            uncertainty_reduction,
            risk_relevance: 1.0,
            estimated_cpu_ms: 10,
            estimated_tokens: usage.text_tokens,
            estimated_capture_bytes: 16_384,
        },
        estimated_usage: usage,
    }
}

fn usage(
    latency_ms: u64,
    text_tokens: usize,
    image_regions: usize,
    chromium_spawns: u32,
) -> PerceptionBudgetUsage {
    PerceptionBudgetUsage {
        latency_ms,
        text_tokens,
        image_regions,
        chromium_spawns,
    }
}

#[test]
fn cheapest_sufficient_action_stays_within_the_cycle_contract_without_escalation() {
    let candidates = vec![
        candidate(
            "viewport",
            PerceptionActionKind::ViewportCapture,
            0.9,
            usage(800, 500, 2, 0),
        ),
        candidate(
            "region",
            PerceptionActionKind::RegionCapture,
            0.8,
            usage(120, 100, 1, 0),
        ),
    ];

    let plan = plan_budgeted_perception_cycle(
        &candidates,
        &budget(),
        &PerceptionCycleSignals::default(),
    );

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].action.id, "region");
    assert_eq!(
        plan.budget_decision.status,
        PerceptionBudgetDecisionStatus::WithinBudget
    );
    assert_eq!(plan.budget_decision.budget_escalation_reason, None);
}

#[test]
fn crossing_the_cycle_budget_without_evidence_for_escalation_is_rejected() {
    let candidates = vec![candidate(
        "viewport",
        PerceptionActionKind::ViewportCapture,
        1.0,
        usage(1_600, 200, 1, 0),
    )];

    let plan = plan_budgeted_perception_cycle(
        &candidates,
        &budget(),
        &PerceptionCycleSignals::default(),
    );

    assert!(plan.actions.is_empty());
    assert_eq!(plan.rejected.len(), 1);
    assert_eq!(
        plan.rejected[0].reason,
        PerceptionPlanRejectionReason::BudgetExceededWithoutAuthorizedEscalation
    );
}

#[test]
fn insufficient_evidence_authorizes_one_bounded_overrun_and_records_the_reason() {
    let candidates = vec![
        candidate(
            "region",
            PerceptionActionKind::RegionCapture,
            1.0,
            usage(1_600, 200, 1, 0),
        ),
        candidate(
            "viewport",
            PerceptionActionKind::ViewportCapture,
            0.9,
            usage(100, 100, 1, 0),
        ),
    ];
    let signals = PerceptionCycleSignals {
        insufficient_evidence: true,
        ..Default::default()
    };

    let plan = plan_budgeted_perception_cycle(&candidates, &budget(), &signals);

    assert_eq!(
        plan.actions.len(),
        1,
        "planner must stop after the first budget overrun"
    );
    assert_eq!(plan.actions[0].action.id, "region");
    assert_eq!(
        plan.budget_decision.status,
        PerceptionBudgetDecisionStatus::Escalated
    );
    assert_eq!(
        plan.budget_decision.budget_escalation_reason,
        Some(BudgetEscalationReason::InsufficientEvidence)
    );
}

#[test]
fn chromium_is_not_unlocked_by_deep_mode_without_browser_specific_suspicion() {
    let candidates = vec![candidate(
        "chromium",
        PerceptionActionKind::ChromiumEscalation,
        1.0,
        usage(300, 100, 0, 1),
    )];
    let signals = PerceptionCycleSignals {
        explicit_deep_mode: true,
        ..Default::default()
    };

    let plan = plan_budgeted_perception_cycle(&candidates, &budget(), &signals);

    assert!(plan.actions.is_empty());
    assert_eq!(
        plan.rejected[0].reason,
        PerceptionPlanRejectionReason::ChromiumRequiresBrowserSpecificSuspicion
    );
}

#[test]
fn browser_specific_suspicion_can_authorize_exactly_one_chromium_spawn() {
    let candidates = vec![candidate(
        "chromium",
        PerceptionActionKind::ChromiumEscalation,
        1.0,
        usage(300, 100, 0, 0),
    )];
    let signals = PerceptionCycleSignals {
        browser_specific_suspicion: true,
        ..Default::default()
    };

    let plan = plan_budgeted_perception_cycle(&candidates, &budget(), &signals);

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.budget_decision.usage.chromium_spawns, 1);
    assert_eq!(
        plan.budget_decision.budget_escalation_reason,
        Some(BudgetEscalationReason::BrowserSpecificSuspicion)
    );
}

#[test]
fn planner_chooses_escalation_reason_deterministically_from_cycle_signals() {
    let candidates = vec![candidate(
        "region",
        PerceptionActionKind::RegionCapture,
        1.0,
        usage(1_600, 100, 1, 0),
    )];
    let signals = PerceptionCycleSignals {
        critical_issue: true,
        explicit_deep_mode: true,
        insufficient_evidence: true,
        browser_specific_suspicion: true,
    };

    let plan = plan_budgeted_perception_cycle(&candidates, &budget(), &signals);

    assert_eq!(
        plan.budget_decision.budget_escalation_reason,
        Some(BudgetEscalationReason::CriticalIssue)
    );
}

#[test]
fn equal_candidate_sets_produce_the_same_plan_regardless_of_input_order() {
    let left = candidate(
        "a",
        PerceptionActionKind::RegionCapture,
        0.8,
        usage(100, 100, 1, 0),
    );
    let right = candidate(
        "b",
        PerceptionActionKind::RegionCapture,
        0.8,
        usage(100, 100, 1, 0),
    );

    let forward = plan_budgeted_perception_cycle(
        &[left.clone(), right.clone()],
        &budget(),
        &PerceptionCycleSignals::default(),
    );
    let reversed = plan_budgeted_perception_cycle(
        &[right, left],
        &budget(),
        &PerceptionCycleSignals::default(),
    );

    let forward_ids: Vec<_> = forward
        .actions
        .iter()
        .map(|item| item.action.id.as_str())
        .collect();
    let reversed_ids: Vec<_> = reversed
        .actions
        .iter()
        .map(|item| item.action.id.as_str())
        .collect();
    assert_eq!(forward_ids, reversed_ids);
    assert_eq!(forward.budget_decision, reversed.budget_decision);
}
