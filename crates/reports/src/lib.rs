#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use localview_diagnostics::DiagnosticReport;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalViewReport {
    pub title: String,
    pub generated_at: String,
    pub project: String,
    pub route: String,
    pub viewport: Option<(u32, u32)>,
    pub diagnostics: DiagnosticReport,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CiStatus {
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    NotRequested,
    Captured,
    Unavailable,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BaselineStatus {
    NotCompared,
    Match,
    Changed,
    Incompatible,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportIdentity {
    pub project: String,
    pub session_id: String,
    pub revision: Option<String>,
    pub state_identity: String,
    pub route: String,
    pub viewport: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationReport {
    pub verdict: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineReport {
    pub status: BaselineStatus,
    pub baseline_hash: Option<String>,
    pub current_hash: Option<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaptureReport {
    pub status: CaptureStatus,
    pub evidence_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitReport {
    pub available: bool,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    pub changed_files: Vec<String>,
    pub relevant_source_files: Vec<String>,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiGateReport {
    pub status: CiStatus,
    pub hard_failures: usize,
    pub heuristic_findings: usize,
    pub subjective_findings: usize,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessReport {
    pub schema_version: u32,
    pub generated_at: String,
    pub identity: ReportIdentity,
    pub evidence_classes: Vec<String>,
    pub diagnostics: DiagnosticReport,
    pub verification: VerificationReport,
    pub baseline: BaselineReport,
    pub capture: CaptureReport,
    pub git: GitReport,
    pub evidence_ids: Vec<String>,
    pub incomplete_reasons: Vec<String>,
    pub ci: CiGateReport,
    pub metadata: BTreeMap<String, String>,
}

pub fn render_headless_json(report: &HeadlessReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn render_headless_markdown(report: &HeadlessReport) -> String {
    let identity = &report.identity;
    let mut output = format!(
        "# LocalView Headless Report\n\n**Project:** {}  \n**Session:** {}  \n**Route:** {}  \n**State:** {}  \n**Generated:** {}\n\n",
        escape_markdown(&identity.project),
        escape_markdown(&identity.session_id),
        escape_markdown(&identity.route),
        escape_markdown(&identity.state_identity),
        escape_markdown(&report.generated_at),
    );
    if let Some(revision) = &identity.revision {
        output.push_str(&format!("**Revision:** {}  \n", escape_markdown(revision)));
    }
    if let Some((width, height)) = identity.viewport {
        output.push_str(&format!("**Viewport:** {width}×{height}  \n"));
    }
    output.push_str(&format!(
        "\n## CI result\n\nStatus: **{:?}**  \nHard deterministic failures: **{}**  \nHeuristic findings: **{}**  \nSubjective findings: **{}**\n\n",
        report.ci.status,
        report.ci.hard_failures,
        report.ci.heuristic_findings,
        report.ci.subjective_findings,
    ));
    if !report.ci.reasons.is_empty() {
        output.push_str("### Gate reasons\n\n");
        for reason in &report.ci.reasons {
            output.push_str(&format!("- {}\n", escape_markdown(reason)));
        }
        output.push('\n');
    }
    output.push_str(&format!(
        "## Verification\n\nVerdict: **{}**\n\n",
        escape_markdown(&report.verification.verdict)
    ));
    for reason in &report.verification.reasons {
        output.push_str(&format!("- {}\n", escape_markdown(reason)));
    }
    output.push_str(&format!(
        "\n## Findings\n\nDeterministic: **{}** · Heuristic: **{}** · Subjective: **{}**\n\n",
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective
    ));
    for issue in &report.diagnostics.issues {
        output.push_str(&format!(
            "### {} / {}\n\n{}\n\n- Severity: {}\n- Confidence: {}%\n- Class: {:?}\n\n",
            escape_markdown(&issue.category),
            escape_markdown(&issue.code),
            escape_markdown(&issue.message),
            issue.severity,
            issue.confidence,
            issue.class,
        ));
    }
    output.push_str(&format!(
        "## Visual capture\n\nStatus: **{:?}**\n\n",
        report.capture.status
    ));
    if let Some(reason) = &report.capture.reason {
        output.push_str(&format!("{}\n\n", escape_markdown(reason)));
    }
    output.push_str(&format!(
        "## Baseline\n\nStatus: **{:?}**\n\n",
        report.baseline.status
    ));
    for reason in &report.baseline.reasons {
        output.push_str(&format!("- {}\n", escape_markdown(reason)));
    }
    output.push_str("\n## Evidence\n\n");
    if report.evidence_ids.is_empty() {
        output.push_str("No private-safe evidence IDs were retained in this report.\n");
    } else {
        for evidence in &report.evidence_ids {
            output.push_str(&format!("- {}\n", escape_markdown(evidence)));
        }
    }
    if !report.incomplete_reasons.is_empty() {
        output.push_str("\n## Incomplete / inconclusive\n\n");
        for reason in &report.incomplete_reasons {
            output.push_str(&format!("- {}\n", escape_markdown(reason)));
        }
    }
    output.push_str("\n## Git\n\n");
    if report.git.available {
        output.push_str(&format!(
            "HEAD: {}  \nBranch: {}  \nDirty: {}\n",
            report.git.head.as_deref().map(escape_markdown).unwrap_or_else(|| "unavailable".into()),
            report.git.branch.as_deref().map(escape_markdown).unwrap_or_else(|| "detached".into()),
            report.git.dirty.map(|value| value.to_string()).unwrap_or_else(|| "unknown".into()),
        ));
    } else {
        output.push_str(&format!(
            "Git unavailable: {}\n",
            report.git.unavailable_reason.as_deref().map(escape_markdown).unwrap_or_else(|| "unknown".into())
        ));
    }
    output
}

pub fn render_headless_html(report: &HeadlessReport) -> String {
    let findings = report
        .diagnostics
        .issues
        .iter()
        .map(|issue| {
            format!(
                "<article><div class=\"meta\">{} · {}% · {:?}</div><h3>{}</h3><p>{}</p></article>",
                escape(&issue.category),
                issue.confidence,
                issue.class,
                escape(&issue.code),
                escape(&issue.message),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let incomplete = report
        .incomplete_reasons
        .iter()
        .map(|reason| format!("<li>{}</li>", escape(reason)))
        .collect::<Vec<_>>()
        .join("");
    let evidence = report
        .evidence_ids
        .iter()
        .map(|id| format!("<li><code>{}</code></li>", escape(id)))
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>LocalView Headless Report</title><style>body{{font:14px system-ui;max-width:980px;margin:40px auto;padding:0 24px;background:#0b0e13;color:#e8edf3}}article,section{{border:1px solid #26303d;border-radius:10px;padding:16px;margin:12px 0;background:#11161e}}.meta{{font:11px ui-monospace;color:#8290a4}}h1,h2,h3{{letter-spacing:-.02em}}code{{color:#9fc7ee}}</style></head><body><h1>LocalView Headless Report</h1><section><p><strong>Project:</strong> {}</p><p><strong>Session:</strong> <code>{}</code></p><p><strong>Route:</strong> <code>{}</code></p><p><strong>State:</strong> <code>{}</code></p><p><strong>Revision:</strong> <code>{}</code></p></section><section><h2>CI result</h2><p>{:?} · hard failures {} · heuristic {} · subjective {}</p></section><section><h2>Verification</h2><p>{}</p></section><section><h2>Visual capture</h2><p>{:?}</p></section><section><h2>Baseline</h2><p>{:?}</p></section><h2>Findings</h2>{}<section><h2>Evidence IDs</h2><ul>{}</ul></section><section><h2>Incomplete / inconclusive</h2><ul>{}</ul></section></body></html>"#,
        escape(&report.identity.project),
        escape(&report.identity.session_id),
        escape(&report.identity.route),
        escape(&report.identity.state_identity),
        escape(report.identity.revision.as_deref().unwrap_or("unavailable")),
        report.ci.status,
        report.ci.hard_failures,
        report.ci.heuristic_findings,
        report.ci.subjective_findings,
        escape(&report.verification.verdict),
        report.capture.status,
        report.baseline.status,
        findings,
        evidence,
        incomplete,
    )
}

fn escape_markdown(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '(' | ')' | '#' | '+' | '-' | '.' | '!' | '|') {
            output.push('\\');
        }
        output.push(ch);
    }
    output
}

pub fn render_json(report: &LocalViewReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn render_markdown(report: &LocalViewReport) -> String {
    let mut output = format!(
        "# {}\n\n**Project:** `{}`  \n**Route:** `{}`  \n**Generated:** {}\n\n",
        report.title, report.project, report.route, report.generated_at
    );
    if let Some((w, h)) = report.viewport {
        output.push_str(&format!("**Viewport:** {w}×{h}\n\n"));
    }
    output.push_str(&format!(
        "## Findings\n\nDeterministic: **{}** · Heuristic: **{}** · Subjective: **{}**\n\n",
        report.diagnostics.deterministic, report.diagnostics.heuristic, report.diagnostics.subjective
    ));
    if report.diagnostics.issues.is_empty() {
        output.push_str("No findings were recorded.\n");
    } else {
        for issue in &report.diagnostics.issues {
            output.push_str(&format!(
                "### [{}] {}\n\n{}\n\n- Severity: {}\n- Confidence: {}%\n- Class: `{:?}`\n",
                issue.category, issue.code, issue.message, issue.severity, issue.confidence, issue.class
            ));
            if let Some(evidence) = &issue.evidence {
                output.push_str(&format!("- Evidence: `{}`\n", evidence.replace('`', "'")));
            }
            output.push('\n');
        }
    }
    output
}

pub fn render_html(report: &LocalViewReport) -> String {
    let findings = report.diagnostics.issues.iter().map(|issue| format!(
        "<article><div class=meta>{} · {}% · {:?}</div><h3>{}</h3><p>{}</p></article>",
        escape(&issue.category), issue.confidence, issue.class, escape(&issue.code), escape(&issue.message)
    )).collect::<Vec<_>>().join("");
    format!(r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>{}</title><style>body{{font:14px system-ui;max-width:980px;margin:40px auto;padding:0 24px;background:#0b0e13;color:#e8edf3}}article{{border:1px solid #26303d;border-radius:10px;padding:16px;margin:12px 0;background:#11161e}}.meta{{font:11px ui-monospace;color:#8290a4}}h1,h3{{letter-spacing:-.02em}}code{{color:#9fc7ee}}</style></head><body><h1>{}</h1><p><code>{}</code> · <code>{}</code></p><p>Deterministic: {} · Heuristic: {} · Subjective: {}</p>{}</body></html>"#,
        escape(&report.title), escape(&report.title), escape(&report.project), escape(&report.route),
        report.diagnostics.deterministic, report.diagnostics.heuristic, report.diagnostics.subjective, findings
    )
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

    fn headless_fixture(project: &str) -> HeadlessReport {
        HeadlessReport {
            schema_version: 1,
            generated_at: "2026-09-21T00:00:00Z".into(),
            identity: ReportIdentity {
                project: project.into(),
                session_id: "session".into(),
                revision: Some("abc".into()),
                state_identity: "sha256:state".into(),
                route: "/".into(),
                viewport: Some((1280, 720)),
            },
            evidence_classes: vec!["semantic".into()],
            diagnostics: DiagnosticReport::default(),
            verification: VerificationReport {
                verdict: "pass".into(),
                reasons: vec![],
            },
            baseline: BaselineReport {
                status: BaselineStatus::NotCompared,
                baseline_hash: None,
                current_hash: None,
                reasons: vec![],
            },
            capture: CaptureReport {
                status: CaptureStatus::NotRequested,
                evidence_id: None,
                reason: None,
            },
            git: GitReport {
                available: false,
                head: None,
                branch: None,
                dirty: None,
                changed_files: vec![],
                relevant_source_files: vec![],
                unavailable_reason: Some("not a git repo".into()),
            },
            evidence_ids: vec![],
            incomplete_reasons: vec![],
            ci: CiGateReport {
                status: CiStatus::Passed,
                hard_failures: 0,
                heuristic_findings: 0,
                subjective_findings: 0,
                reasons: vec![],
            },
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn headless_json_carries_schema_and_identity() {
        let rendered = render_headless_json(&headless_fixture("demo")).expect("json");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("parse");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["identity"]["state_identity"], "sha256:state");
    }

    #[test]
    fn headless_markdown_escapes_project_content() {
        let rendered = render_headless_markdown(&headless_fixture("# injected <script>"));
        assert!(rendered.contains("\\# injected \\<script\\>"));
        assert!(!rendered.contains("\n# injected <script>"));
    }

    #[test]
    fn headless_html_escapes_all_project_content() {
        let rendered = render_headless_html(&headless_fixture("<script>alert('x')</script>"));
        assert!(rendered.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
        assert!(!rendered.contains("<script>alert"));
    }

    #[test]
    fn html_escapes_project_name() {
        let report = LocalViewReport {
            title: "Audit".into(), generated_at: "now".into(), project: "<demo>".into(), route: "/".into(), viewport: None,
            diagnostics: DiagnosticReport::default(), metadata: BTreeMap::new(),
        };
        assert!(render_html(&report).contains("&lt;demo&gt;"));
    }
}
