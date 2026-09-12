#![forbid(unsafe_code)]

use localview_a11y::A11yFinding;
use localview_layout::{LayoutIssue, Severity};
use localview_network::{NetworkFinding, NetworkIssueKind};
use localview_performance::PerformanceFinding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticClass { Deterministic, Heuristic, Subjective }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticIssue {
    pub category: String,
    pub code: String,
    pub message: String,
    pub severity: u8,
    pub confidence: u8,
    pub class: DiagnosticClass,
    pub refs: Vec<String>,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiagnosticReport {
    pub issues: Vec<DiagnosticIssue>,
    pub deterministic: usize,
    pub heuristic: usize,
    pub subjective: usize,
}

pub fn assemble(
    layout: &[LayoutIssue],
    network: &[NetworkFinding],
    accessibility: &[A11yFinding],
    performance: &[PerformanceFinding],
) -> DiagnosticReport {
    let mut issues = Vec::new();

    issues.extend(layout.iter().map(|issue| DiagnosticIssue {
        category: "layout".into(),
        code: issue.code.clone(),
        message: issue.message.clone(),
        severity: match issue.severity { Severity::Info => 1, Severity::Warning => 2, Severity::Error => 3 },
        confidence: (issue.confidence.clamp(0.0, 1.0) * 100.0).round() as u8,
        class: if issue.confidence >= 0.99 { DiagnosticClass::Deterministic } else { DiagnosticClass::Heuristic },
        refs: issue.refs.clone(),
        evidence: Some(issue.evidence.clone()),
    }));

    issues.extend(network.iter().map(|issue| DiagnosticIssue {
        category: "network".into(),
        code: format!("{:?}", issue.kind).to_ascii_lowercase(),
        message: issue.message.clone(),
        severity: match issue.kind { NetworkIssueKind::Failed | NetworkIssueKind::Cors => 3, _ => 2 },
        confidence: issue.confidence,
        class: if issue.confidence >= 95 { DiagnosticClass::Deterministic } else { DiagnosticClass::Heuristic },
        refs: issue.request_ids.clone(),
        evidence: None,
    }));

    issues.extend(accessibility.iter().map(|issue| DiagnosticIssue {
        category: "accessibility".into(),
        code: issue.code.clone(),
        message: issue.message.clone(),
        severity: 2,
        confidence: issue.confidence,
        class: if issue.deterministic { DiagnosticClass::Deterministic } else { DiagnosticClass::Heuristic },
        refs: vec![issue.reference.clone()],
        evidence: None,
    }));

    issues.extend(performance.iter().map(|issue| DiagnosticIssue {
        category: "performance".into(),
        code: issue.code.clone(),
        message: issue.message.clone(),
        severity: issue.severity,
        confidence: 100,
        class: DiagnosticClass::Deterministic,
        refs: Vec::new(),
        evidence: None,
    }));

    issues.sort_by_key(|issue| (std::cmp::Reverse(issue.severity), std::cmp::Reverse(issue.confidence)));
    let deterministic = issues.iter().filter(|i| i.class == DiagnosticClass::Deterministic).count();
    let heuristic = issues.iter().filter(|i| i.class == DiagnosticClass::Heuristic).count();
    let subjective = issues.iter().filter(|i| i.class == DiagnosticClass::Subjective).count();
    DiagnosticReport { issues, deterministic, heuristic, subjective }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_inputs_produce_empty_report() {
        let report = assemble(&[], &[], &[], &[]);
        assert!(report.issues.is_empty());
        assert_eq!(report.deterministic, 0);
    }
}
