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
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escapes_project_name() {
        let report = LocalViewReport {
            title: "Audit".into(), generated_at: "now".into(), project: "<demo>".into(), route: "/".into(), viewport: None,
            diagnostics: DiagnosticReport::default(), metadata: BTreeMap::new(),
        };
        assert!(render_html(&report).contains("&lt;demo&gt;"));
    }
}
