#![forbid(unsafe_code)]

use localview_planner::{BudgetedPerceptionPlan, PerceptionActionKind};
use localview_token_budget::{BudgetEscalationReason, PerceptionBudgetDecisionStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum EngineTier {
    Static = 0,
    Lightweight = 1,
    NativeWebView = 2,
    Chromium = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineNeeds {
    pub source_only: bool,
    pub javascript: bool,
    pub interaction: bool,
    pub screenshot: bool,
    pub exact_platform_render: bool,
    pub chrome_compatibility: bool,
    pub devtools_trace: bool,
    pub advanced_emulation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineDecision {
    pub tier: EngineTier,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EngineAdmissionError {
    PlannerAuthorizationRequired,
    InvalidPlannerAuthorization,
}

pub fn choose_engine(needs: &EngineNeeds) -> EngineDecision {
    let mut reasons = Vec::new();
    let tier = if needs.chrome_compatibility || needs.devtools_trace || needs.advanced_emulation {
        reasons.push("browser-specific capability requested".into());
        EngineTier::Chromium
    } else if needs.screenshot || needs.exact_platform_render {
        reasons.push("human-visible native rendering required".into());
        EngineTier::NativeWebView
    } else if needs.javascript || needs.interaction {
        reasons.push("semantic runtime execution required".into());
        EngineTier::Lightweight
    } else {
        reasons.push("static/source inspection is sufficient".into());
        EngineTier::Static
    };
    EngineDecision { tier, reasons }
}

pub fn choose_engine_authorized(
    needs: &EngineNeeds,
    perception_plan: Option<&BudgetedPerceptionPlan>,
) -> Result<EngineDecision, EngineAdmissionError> {
    let mut decision = choose_engine(needs);
    if decision.tier != EngineTier::Chromium {
        return Ok(decision);
    }

    let plan = perception_plan.ok_or(EngineAdmissionError::PlannerAuthorizationRequired)?;
    if plan.actions.len() != 1
        || plan.actions[0].action.kind != PerceptionActionKind::ChromiumEscalation
    {
        return Err(EngineAdmissionError::PlannerAuthorizationRequired);
    }

    if plan.budget_decision.usage.chromium_spawns != 1 {
        return Err(EngineAdmissionError::InvalidPlannerAuthorization);
    }

    match plan.budget_decision.status {
        PerceptionBudgetDecisionStatus::WithinBudget => {
            if plan.budget_decision.budget_escalation_reason.is_some() {
                return Err(EngineAdmissionError::InvalidPlannerAuthorization);
            }
        }
        PerceptionBudgetDecisionStatus::Escalated => {
            if plan.budget_decision.budget_escalation_reason
                != Some(BudgetEscalationReason::BrowserSpecificSuspicion)
            {
                return Err(EngineAdmissionError::InvalidPlannerAuthorization);
            }
        }
    }

    decision.reasons.push(format!(
        "planner-authorized Tier-3 Chromium action: {}",
        plan.actions[0].action.id
    ));
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_is_not_default() {
        assert_eq!(
            choose_engine(&EngineNeeds {
                javascript: true,
                ..Default::default()
            })
            .tier,
            EngineTier::Lightweight
        );
    }
}
