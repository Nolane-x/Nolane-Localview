#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_diagnostics::{DiagnosticClass, DiagnosticReport};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportStatus {
    Passed,
    Failed,
    Inconclusive,
    InfrastructureError,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaselineComparisonStatus {
    NotRequested,
    Created,
    Match,
    Changed,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GitAnnotation {
    pub available: bool,
    pub revision: Option<String>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    pub changed_files: Vec<String>,
    pub relevant_source_files: Vec<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineComparison {
    pub status: BaselineComparisonStatus,
    pub baseline_hash: Option<String>,
    pub candidate_hash: Option<String>,
    pub reasons: Vec<String>,
}

impl Default for BaselineComparison {
    fn default() -> Self {
        Self {
            status: BaselineComparisonStatus::NotRequested,
            baseline_hash: None,
            candidate_hash: None,
            reasons: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactReference {
    pub kind: String,
    pub storage_id: String,
    pub content_hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalViewReport {
    pub schema_version: u32,
    pub title: String,
    pub generated_at: String,
    pub status: ReportStatus,
    pub project: String,
    pub project_key: String,
    pub session_id: String,
    pub revision: Option<String>,
    pub state_identity: String,
    pub route: String,
    pub viewport: Option<(u32, u32)>,
    pub evidence_classes: BTreeMap<String, usize>,
    pub evidence_ids: Vec<String>,
    pub diagnostics: DiagnosticReport,
    pub verification: Value,
    pub baseline: BaselineComparison,
    pub artifacts: Vec<ArtifactReference>,
    pub incomplete_reasons: Vec<String>,
    pub inconclusive_reasons: Vec<String>,
    pub git: GitAnnotation,
    pub metadata: BTreeMap<String, String>,
}

impl LocalViewReport {
    pub fn normalize(&mut self) {
        self.evidence_ids.sort();
        self.evidence_ids.dedup();
        self.incomplete_reasons.sort();
        self.incomplete_reasons.dedup();
        self.inconclusive_reasons.sort();
        self.inconclusive_reasons.dedup();
        self.git.changed_files.sort();
        self.git.changed_files.dedup();
        self.git.relevant_source_files.sort();
        self.git.relevant_source_files.dedup();
        self.artifacts.sort_by(|a, b| {
            (&a.kind, &a.content_hash, &a.storage_id).cmp(&(&b.kind, &b.content_hash, &b.storage_id))
        });
    }

    pub fn deterministic_failure_count(&self) -> usize {
        self.diagnostics
            .issues
            .iter()
            .filter(|issue| issue.class == DiagnosticClass::Deterministic)
            .count()
    }

    pub fn relevant_source_files(&self) -> BTreeSet<String> {
        self.git.relevant_source_files.iter().cloned().collect()
    }
}

pub fn render_json(report: &LocalViewReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn render_markdown(report: &LocalViewReport) -> String {
    let mut output = format!(
        "# {}\n\n**Status:** `{}`  \n**Project:** `{}`  \n**Session:** `{}`  \n**Route:** `{}`  \n**Generated:** {}\n\n",
        markdown_text(&report.title),
        status_name(report.status),
        markdown_code(&report.project),
        markdown_code(&report.session_id),
        markdown_code(&report.route),
        markdown_text(&report.generated_at),
    );
    if let Some((w, h)) = report.viewport {
        output.push_str(&format!("**Viewport:** {w}×{h}  \n"));
    }
    if let Some(revision) = &report.revision {
        output.push_str(&format!("**Revision:** `{}`  \n", markdown_code(revision)));
    }
    output.push_str(&format!(
        "**State:** `{}`\n\n## Findings\n\nDeterministic: **{}** · Heuristic: **{}** · Subjective: **{}**\n\n",
        markdown_code(&report.state_identity),
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective
    ));
    if report.diagnostics.issues.is_empty() {
        output.push_str("No findings were recorded.\n\n");
    } else {
        for issue in &report.diagnostics.issues {
            output.push_str(&format!(
                "### [{}] {}\n\n{}\n\n- Severity: {}\n- Confidence: {}%\n- Class: `{:?}`\n",
                markdown_text(&issue.category),
                markdown_text(&issue.code),
                markdown_text(&issue.message),
                issue.severity,
                issue.confidence,
                issue.class
            ));
            if let Some(evidence) = &issue.evidence {
                output.push_str(&format!("- Evidence: `{}`\n", markdown_code(evidence)));
            }
            output.push('\n');
        }
    }

    output.push_str("## Baseline\n\n");
    output.push_str(&format!(
        "Status: `{}`\n\n",
        baseline_status_name(report.baseline.status)
    ));
    for reason in &report.baseline.reasons {
        output.push_str(&format!("- {}\n", markdown_text(reason)));
    }

    output.push_str("\n## Verification\n\n```json\n");
    let verification = serde_json::to_string_pretty(&report.verification)
        .unwrap_or_else(|_| "null".to_owned())
        .replace("```", "` ` `");
    output.push_str(&verification);
    output.push_str("\n```\n");

    if !report.incomplete_reasons.is_empty() || !report.inconclusive_reasons.is_empty() {
        output.push_str("\n## Incomplete / inconclusive\n\n");
        for reason in report
            .incomplete_reasons
            .iter()
            .chain(report.inconclusive_reasons.iter())
        {
            output.push_str(&format!("- {}\n", markdown_text(reason)));
        }
    }

    output
}

pub fn render_html(report: &LocalViewReport) -> String {
    let findings = report
        .diagnostics
        .issues
        .iter()
        .map(|issue| {
            format!(
                "<article><div class=meta>{} · {}% · {:?}</div><h3>{}</h3><p>{}</p></article>",
                escape(&issue.category),
                issue.confidence,
                issue.class,
                escape(&issue.code),
                escape(&issue.message)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let reasons = report
        .incomplete_reasons
        .iter()
        .chain(report.inconclusive_reasons.iter())
        .map(|reason| format!("<li>{}</li>", escape(reason)))
        .collect::<Vec<_>>()
        .join("");
    let revision = report.revision.as_deref().unwrap_or("unavailable");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>{}</title><style>body{{font:14px system-ui;max-width:980px;margin:40px auto;padding:0 24px;background:#0b0e13;color:#e8edf3}}article{{border:1px solid #26303d;border-radius:10px;padding:16px;margin:12px 0;background:#11161e}}.meta{{font:11px ui-monospace;color:#8290a4}}h1,h3{{letter-spacing:-.02em}}code{{color:#9fc7ee}}</style></head><body><h1>{}</h1><p>Status: <code>{}</code></p><p><code>{}</code> · <code>{}</code> · <code>{}</code></p><p>Revision: <code>{}</code> · State: <code>{}</code></p><p>Deterministic: {} · Heuristic: {} · Subjective: {}</p>{}<h2>Baseline</h2><p><code>{}</code></p><h2>Incomplete / inconclusive</h2><ul>{}</ul></body></html>"#,
        escape(&report.title),
        escape(&report.title),
        status_name(report.status),
        escape(&report.project),
        escape(&report.session_id),
        escape(&report.route),
        escape(revision),
        escape(&report.state_identity),
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective,
        findings,
        baseline_status_name(report.baseline.status),
        reasons
    )
}

fn status_name(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Passed => "passed",
        ReportStatus::Failed => "failed",
        ReportStatus::Inconclusive => "inconclusive",
        ReportStatus::InfrastructureError => "infrastructure_error",
    }
}

fn baseline_status_name(status: BaselineComparisonStatus) -> &'static str {
    match status {
        BaselineComparisonStatus::NotRequested => "not_requested",
        BaselineComparisonStatus::Created => "created",
        BaselineComparisonStatus::Match => "match",
        BaselineComparisonStatus::Changed => "changed",
        BaselineComparisonStatus::Incompatible => "incompatible",
    }
}

fn markdown_text(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('\r', " ")
        .replace('\n', " ")
        .replace('#', "\\#")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn markdown_code(input: &str) -> String {
    input
        .replace('\r', " ")
        .replace('\n', " ")
        .replace('`', "'")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> LocalViewReport {
        LocalViewReport {
            schema_version: 1,
            title: "Audit".into(),
            generated_at: "now".into(),
            status: ReportStatus::Passed,
            project: "<demo>".into(),
            project_key: "demo".into(),
            session_id: "s1".into(),
            revision: Some("abc".into()),
            state_identity: "sha256:state".into(),
            route: "/".into(),
            viewport: None,
            evidence_classes: BTreeMap::new(),
            evidence_ids: Vec::new(),
            diagnostics: DiagnosticReport::default(),
            verification: Value::Null,
            baseline: BaselineComparison::default(),
            artifacts: Vec::new(),
            incomplete_reasons: Vec::new(),
            inconclusive_reasons: Vec::new(),
            git: GitAnnotation::default(),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn json_report_has_stable_wave8_schema_fields() {
        let rendered = render_json(&report()).expect("render JSON");
        let value: Value = serde_json::from_str(&rendered).expect("parse JSON");
        assert_eq!(
            value.get("schema_version").and_then(Value::as_u64),
            Some(1)
        );
        assert!(value.get("state_identity").is_some());
        assert!(value.get("diagnostics").is_some());
        assert!(value.get("verification").is_some());
        assert!(value.get("baseline").is_some());
        assert!(value.get("git").is_some());
    }

    #[test]
    fn html_escapes_all_project_content() {
        let mut value = report();
        value.title = "<script>alert('x')</script>".into();
        value.route = "<img src=x onerror=1>".into();
        value.incomplete_reasons.push("<svg/onload=1>".into());
        let rendered = render_html(&value);
        assert!(!rendered.contains("<script>alert"));
        assert!(!rendered.contains("<img src=x"));
        assert!(!rendered.contains("<svg/onload"));
    }

    #[test]
    fn markdown_flattens_headings_and_fences_from_project_content() {
        let mut value = report();
        value.title = "x\n# injected".into();
        value.project = "```\nsecret".into();
        let rendered = render_markdown(&value);
        assert!(!rendered.contains("\n# injected"));
        assert!(!rendered.contains("**Project:** `````"));
    }
}
