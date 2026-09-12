use localview_token_budget::{
    evaluate_perception_budget, BudgetDimension, BudgetEscalationReason,
    PerceptionBudgetContract, PerceptionBudgetDecisionStatus, PerceptionBudgetUsage,
};

fn contract() -> PerceptionBudgetContract {
    PerceptionBudgetContract {
        latency_ms: 1_500,
        text_tokens: 800,
        image_regions: 2,
        chromium_spawns: 0,
    }
}

#[test]
fn usage_inside_all_four_dimensions_is_admitted_without_escalation() {
    let usage = PerceptionBudgetUsage {
        latency_ms: 900,
        text_tokens: 420,
        image_regions: 1,
        chromium_spawns: 0,
    };

    let decision = evaluate_perception_budget(&contract(), &usage, None).unwrap();

    assert_eq!(decision.status, PerceptionBudgetDecisionStatus::WithinBudget);
    assert!(decision.exceeded.is_empty());
    assert_eq!(decision.budget_escalation_reason, None);
    assert_eq!(decision.usage, usage);
}

#[test]
fn over_budget_without_an_allowed_reason_fails_closed_and_reports_every_dimension() {
    let usage = PerceptionBudgetUsage {
        latency_ms: 1_501,
        text_tokens: 801,
        image_regions: 3,
        chromium_spawns: 1,
    };

    let violation = evaluate_perception_budget(&contract(), &usage, None).unwrap_err();

    assert_eq!(
        violation.exceeded,
        vec![
            BudgetDimension::LatencyMs,
            BudgetDimension::TextTokens,
            BudgetDimension::ImageRegions,
            BudgetDimension::ChromiumSpawns,
        ]
    );
    assert_eq!(
        violation.to_string(),
        "perception budget exceeded without an allowed escalation reason"
    );
}

#[test]
fn explicit_deep_mode_can_cross_budget_but_reason_is_preserved_in_the_decision() {
    let usage = PerceptionBudgetUsage {
        latency_ms: 2_000,
        text_tokens: 900,
        image_regions: 3,
        chromium_spawns: 0,
    };

    let decision = evaluate_perception_budget(
        &contract(),
        &usage,
        Some(BudgetEscalationReason::ExplicitDeepMode),
    )
    .unwrap();

    assert_eq!(decision.status, PerceptionBudgetDecisionStatus::Escalated);
    assert_eq!(
        decision.budget_escalation_reason,
        Some(BudgetEscalationReason::ExplicitDeepMode)
    );
    assert_eq!(
        decision.exceeded,
        vec![
            BudgetDimension::LatencyMs,
            BudgetDimension::TextTokens,
            BudgetDimension::ImageRegions,
        ]
    );
}

#[test]
fn all_spec_escalation_reasons_are_explicit_and_serializable() {
    let reasons = [
        BudgetEscalationReason::CriticalIssue,
        BudgetEscalationReason::ExplicitDeepMode,
        BudgetEscalationReason::InsufficientEvidence,
        BudgetEscalationReason::BrowserSpecificSuspicion,
    ];

    let json = serde_json::to_value(reasons).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            "critical_issue",
            "explicit_deep_mode",
            "insufficient_evidence",
            "browser_specific_suspicion"
        ])
    );
}

#[test]
fn zero_budget_is_valid_and_blocks_nonzero_work_without_escalation() {
    let zero = PerceptionBudgetContract {
        latency_ms: 0,
        text_tokens: 0,
        image_regions: 0,
        chromium_spawns: 0,
    };
    let usage = PerceptionBudgetUsage {
        latency_ms: 1,
        text_tokens: 0,
        image_regions: 0,
        chromium_spawns: 0,
    };

    let violation = evaluate_perception_budget(&zero, &usage, None).unwrap_err();
    assert_eq!(violation.exceeded, vec![BudgetDimension::LatencyMs]);
}
