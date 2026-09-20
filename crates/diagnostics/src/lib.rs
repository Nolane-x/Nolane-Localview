#![forbid(unsafe_code)]

use localview_a11y::A11yFinding;
use localview_layout::{LayoutIssue, LayoutIssueClass, Severity};
use localview_network::{NetworkFinding, NetworkIssueKind};
use localview_performance::PerformanceFinding;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticClass {
    Deterministic,
    Heuristic,
    Subjective,
}

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
        severity: match issue.severity {
            Severity::Info => 1,
            Severity::Warning => 2,
            Severity::Error => 3,
        },
        confidence: (issue.confidence.clamp(0.0, 1.0) * 100.0).round() as u8,
        class: match issue.class {
            LayoutIssueClass::Deterministic => DiagnosticClass::Deterministic,
            LayoutIssueClass::Heuristic => DiagnosticClass::Heuristic,
        },
        refs: issue.refs.clone(),
        evidence: Some(issue.evidence.clone()),
    }));

    issues.extend(network.iter().map(|issue| DiagnosticIssue {
        category: "network".into(),
        code: format!("{:?}", issue.kind).to_ascii_lowercase(),
        message: issue.message.clone(),
        severity: match issue.kind {
            NetworkIssueKind::Failed | NetworkIssueKind::Cors => 3,
            _ => 2,
        },
        confidence: issue.confidence,
        class: if issue.confidence >= 95 {
            DiagnosticClass::Deterministic
        } else {
            DiagnosticClass::Heuristic
        },
        refs: issue.request_ids.clone(),
        evidence: None,
    }));

    issues.extend(accessibility.iter().map(|issue| DiagnosticIssue {
        category: "accessibility".into(),
        code: issue.code.clone(),
        message: issue.message.clone(),
        severity: 2,
        confidence: issue.confidence,
        class: if issue.deterministic {
            DiagnosticClass::Deterministic
        } else {
            DiagnosticClass::Heuristic
        },
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

    issues.sort_by_key(|issue| {
        (
            std::cmp::Reverse(issue.severity),
            std::cmp::Reverse(issue.confidence),
        )
    });
    let deterministic = issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Deterministic)
        .count();
    let heuristic = issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Heuristic)
        .count();
    let subjective = issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Subjective)
        .count();
    DiagnosticReport {
        issues,
        deterministic,
        heuristic,
        subjective,
    }
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

    #[test]
    fn layout_classification_is_explicit_not_inferred_from_confidence() {
        let layout = vec![
            LayoutIssue {
                code: "deterministic-low-confidence-proof".into(),
                severity: Severity::Warning,
                confidence: 0.75,
                class: LayoutIssueClass::Deterministic,
                refs: vec!["@a".into()],
                message: "test".into(),
                evidence: "observed".into(),
            },
            LayoutIssue {
                code: "heuristic-high-confidence-proof".into(),
                severity: Severity::Warning,
                confidence: 1.0,
                class: LayoutIssueClass::Heuristic,
                refs: vec!["@b".into()],
                message: "test".into(),
                evidence: "inferred".into(),
            },
        ];

        let report = assemble(&layout, &[], &[], &[]);
        assert_eq!(report.deterministic, 1);
        assert_eq!(report.heuristic, 1);
        assert_eq!(report.issues[0].class, DiagnosticClass::Heuristic);
        assert_eq!(report.issues[1].class, DiagnosticClass::Deterministic);
    }
}
