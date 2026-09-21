#![forbid(unsafe_code)]

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use localview_content_addressed::object_hash;
use localview_diagnostics::DiagnosticReport;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const WAVE8_REPORT_SCHEMA_VERSION: u32 = 1;
const MAX_TEXT_BYTES: usize = 4 * 1024;
const MAX_FINDINGS: usize = 512;
const MAX_IDS: usize = 1024;
const MAX_FILES: usize = 256;

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
    Created,
    Match,
    Changed,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineComparison {
    pub status: BaselineComparisonStatus,
    pub baseline_hash: Option<String>,
    pub candidate_hash: Option<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactReference {
    pub kind: String,
    pub storage_id: String,
    pub content_hash: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitAnnotation {
    pub available: bool,
    pub revision: Option<String>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub relevant_source_files: Vec<String>,
    pub unavailable_reason: Option<String>,
}

impl Default for GitAnnotation {
    fn default() -> Self {
        Self {
            available: false,
            revision: None,
            branch: None,
            dirty: None,
            changed_files: Vec::new(),
            relevant_source_files: Vec::new(),
            unavailable_reason: Some("git unavailable".into()),
        }
    }
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
    #[serde(default)]
    pub evidence_classes: BTreeMap<String, usize>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub diagnostics: DiagnosticReport,
    pub verification: Value,
    pub baseline: BaselineComparison,
    #[serde(default)]
    pub artifacts: Vec<ArtifactReference>,
    #[serde(default)]
    pub incomplete_reasons: Vec<String>,
    #[serde(default)]
    pub inconclusive_reasons: Vec<String>,
    #[serde(default)]
    pub git: GitAnnotation,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl LocalViewReport {
    pub fn normalize(&mut self) {
        self.schema_version = 1;
        self.title = bounded_text(&self.title);
        self.generated_at = bounded_text(&self.generated_at);
        self.project = bounded_text(&self.project);
        self.project_key = bounded_identifier(&self.project_key);
        self.session_id = bounded_identifier(&self.session_id);
        self.revision = self.revision.as_deref().map(bounded_identifier);
        self.state_identity = bounded_identifier(&self.state_identity);
        self.route = bounded_route(&self.route);
        self.evidence_ids = bounded_ids(&self.evidence_ids);
        self.incomplete_reasons = bounded_texts(&self.incomplete_reasons, MAX_IDS);
        self.inconclusive_reasons = bounded_texts(&self.inconclusive_reasons, MAX_IDS);

        self.diagnostics.issues.truncate(MAX_FINDINGS);
        for issue in &mut self.diagnostics.issues {
            issue.category = bounded_identifier(&issue.category);
            issue.code = bounded_identifier(&issue.code);
            issue.message = bounded_text(&issue.message);
            issue.refs = bounded_relative_files(&issue.refs);
            issue.evidence = issue.evidence.as_deref().map(bounded_text);
        }

        self.git.revision = self.git.revision.as_deref().map(bounded_identifier);
        self.git.branch = self.git.branch.as_deref().map(bounded_text);
        self.git.changed_files = bounded_relative_files(&self.git.changed_files);
        self.git.relevant_source_files =
            bounded_relative_files(&self.git.relevant_source_files);
        self.git.unavailable_reason =
            self.git.unavailable_reason.as_deref().map(bounded_text);

        self.baseline.baseline_hash =
            self.baseline.baseline_hash.as_deref().map(bounded_identifier);
        self.baseline.candidate_hash =
            self.baseline.candidate_hash.as_deref().map(bounded_identifier);
        self.baseline.reasons = bounded_texts(&self.baseline.reasons, MAX_IDS);

        self.artifacts.truncate(MAX_FILES);
        for artifact in &mut self.artifacts {
            artifact.kind = bounded_identifier(&artifact.kind);
            artifact.storage_id = bounded_identifier(&artifact.storage_id);
            artifact.content_hash = bounded_identifier(&artifact.content_hash);
        }

        sanitize_json_value(&mut self.verification, 0);

        let metadata = std::mem::take(&mut self.metadata);
        self.metadata = metadata
            .into_iter()
            .take(128)
            .filter(|(key, _)| !sensitive_key(key))
            .map(|(key, value)| (bounded_identifier(&key), bounded_text(&value)))
            .collect();
    }
}

pub fn render_json(report: &LocalViewReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

pub fn render_markdown(report: &LocalViewReport) -> String {
    let mut output = format!(
        "# {}\n\n**Project:** `{}`  \n**Route:** `{}`  \n**Generated:** {}\n\n",
        markdown_text(&report.title),
        markdown_code(&report.project),
        markdown_code(&report.route),
        markdown_text(&report.generated_at)
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
    output
}

pub fn render_html(report: &LocalViewReport) -> String {
    let findings = report
        .diagnostics
        .issues
        .iter()
        .map(|issue| {
            format!(
                "<article><div class=\"meta\">{} · {}% · {:?}</div><h3>{}</h3><p>{}</p></article>",
                html_escape(&issue.category),
                issue.confidence,
                issue.class,
                html_escape(&issue.code),
                html_escape(&issue.message)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>{}</title><style>body{{font:14px system-ui;max-width:980px;margin:40px auto;padding:0 24px;background:#0b0e13;color:#e8edf3}}article{{border:1px solid #26303d;border-radius:10px;padding:16px;margin:12px 0;background:#11161e}}.meta{{font:11px ui-monospace;color:#8290a4}}h1,h3{{letter-spacing:-.02em}}code{{color:#9fc7ee}}</style></head><body><h1>{}</h1><p><code>{}</code> · <code>{}</code></p><p>Deterministic: {} · Heuristic: {} · Subjective: {}</p>{}</body></html>"#,
        html_escape(&report.title),
        html_escape(&report.title),
        html_escape(&report.project),
        html_escape(&report.route),
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective,
        findings
    )
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Wave8ReportStatus {
    Passed,
    Failed,
    Inconclusive,
    InfrastructureError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8Identity {
    pub project: String,
    pub project_identity_hash: String,
    pub session_id: String,
    pub revision: Option<String>,
    pub route: String,
    pub viewport: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8DiagnosticCounts {
    pub deterministic: usize,
    pub heuristic: usize,
    pub subjective: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8Finding {
    pub category: String,
    pub code: String,
    pub message: String,
    pub severity: u8,
    pub confidence: u8,
    pub class: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub source_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8VerificationSummary {
    pub verdict: String,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub required_evidence_classes: Vec<String>,
    #[serde(default)]
    pub fresh_evidence_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8BaselineComparison {
    pub baseline_hash: Option<String>,
    pub comparable: bool,
    pub changed: Option<bool>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8EvidenceSummary {
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub evidence_hashes: Vec<String>,
    #[serde(default)]
    pub proof_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8GitAnnotation {
    pub available: bool,
    pub revision: Option<String>,
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub relevant_source_files: Vec<String>,
    pub reason: Option<String>,
}

impl Default for Wave8GitAnnotation {
    fn default() -> Self {
        Self {
            available: false,
            revision: None,
            branch: None,
            dirty: None,
            changed_files: Vec::new(),
            relevant_source_files: Vec::new(),
            reason: Some("git unavailable".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8Report {
    pub schema_version: u32,
    pub title: String,
    pub generated_at: String,
    pub identity: Wave8Identity,
    pub state_identity: String,
    pub diagnostics: Wave8DiagnosticCounts,
    #[serde(default)]
    pub findings: Vec<Wave8Finding>,
    pub verification: Wave8VerificationSummary,
    pub baseline: Wave8BaselineComparison,
    pub evidence: Wave8EvidenceSummary,
    pub git: Wave8GitAnnotation,
    #[serde(default)]
    pub incomplete_reasons: Vec<String>,
    pub status: Wave8ReportStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Wave8ReportBundle {
    pub canonical_hash: String,
    pub json: String,
    pub markdown: String,
    pub html: String,
}

impl Wave8Report {
    pub fn sanitized(&self) -> Self {
        let mut output = self.clone();
        output.schema_version = WAVE8_REPORT_SCHEMA_VERSION;
        output.title = bounded_text(&output.title);
        output.generated_at = bounded_text(&output.generated_at);
        output.state_identity = bounded_identifier(&output.state_identity);
        output.identity.project = bounded_text(&output.identity.project);
        output.identity.project_identity_hash =
            bounded_identifier(&output.identity.project_identity_hash);
        output.identity.session_id = bounded_identifier(&output.identity.session_id);
        output.identity.revision = output.identity.revision.as_deref().map(bounded_identifier);
        output.identity.route = bounded_route(&output.identity.route);

        output.findings.truncate(MAX_FINDINGS);
        for finding in &mut output.findings {
            finding.category = bounded_identifier(&finding.category);
            finding.code = bounded_identifier(&finding.code);
            finding.message = bounded_text(&finding.message);
            finding.class = bounded_identifier(&finding.class);
            finding.evidence_ids = bounded_ids(&finding.evidence_ids);
            finding.source_files = bounded_relative_files(&finding.source_files);
        }

        output.verification.verdict = bounded_identifier(&output.verification.verdict);
        output.verification.reasons = bounded_texts(&output.verification.reasons, MAX_IDS);
        output.verification.required_evidence_classes =
            bounded_ids(&output.verification.required_evidence_classes);
        output.verification.fresh_evidence_classes =
            bounded_ids(&output.verification.fresh_evidence_classes);

        output.baseline.baseline_hash =
            output.baseline.baseline_hash.as_deref().map(bounded_identifier);
        output.baseline.reason = bounded_text(&output.baseline.reason);

        output.evidence.classes = bounded_ids(&output.evidence.classes);
        output.evidence.evidence_ids = bounded_ids(&output.evidence.evidence_ids);
        output.evidence.evidence_hashes = bounded_ids(&output.evidence.evidence_hashes);
        output.evidence.proof_hashes = bounded_ids(&output.evidence.proof_hashes);

        output.git.revision = output.git.revision.as_deref().map(bounded_identifier);
        output.git.branch = output.git.branch.as_deref().map(bounded_text);
        output.git.changed_files = bounded_relative_files(&output.git.changed_files);
        output.git.relevant_source_files =
            bounded_relative_files(&output.git.relevant_source_files);
        output.git.reason = output.git.reason.as_deref().map(bounded_text);

        output.incomplete_reasons = bounded_texts(&output.incomplete_reasons, MAX_IDS);
        output
    }
}

pub fn canonical_wave8_report_hash(report: &Wave8Report) -> String {
    object_hash(&report.sanitized())
}

pub fn produce_wave8_report_bundle(
    report: &Wave8Report,
) -> Result<Wave8ReportBundle, serde_json::Error> {
    let report = report.sanitized();
    let canonical_hash = object_hash(&report);
    let json = serde_json::to_string_pretty(&report)?;
    let markdown = render_wave8_markdown(&report);
    let html = render_wave8_html(&report);
    Ok(Wave8ReportBundle {
        canonical_hash,
        json,
        markdown,
        html,
    })
}

pub fn render_wave8_markdown(report: &Wave8Report) -> String {
    let report = report.sanitized();
    let mut output = String::new();
    output.push_str(&format!("# {}\n\n", markdown_text(&report.title)));
    output.push_str(&format!(
        "**Project:** `{}`  \n**Session:** `{}`  \n**Route:** `{}`  \n",
        markdown_code(&report.identity.project),
        markdown_code(&report.identity.session_id),
        markdown_code(&report.identity.route),
    ));
    if let Some(revision) = &report.identity.revision {
        output.push_str(&format!("**Revision:** `{}`  \n", markdown_code(revision)));
    }
    if let Some((width, height)) = report.identity.viewport {
        output.push_str(&format!("**Viewport:** {width}×{height}  \n"));
    }
    output.push_str(&format!(
        "**State:** `{}`  \n**Status:** `{:?}`\n\n",
        markdown_code(&report.state_identity),
        report.status
    ));

    output.push_str("## Findings\n\n");
    output.push_str(&format!(
        "Deterministic: **{}** · Heuristic: **{}** · Subjective: **{}**\n\n",
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective
    ));
    if report.findings.is_empty() {
        output.push_str("No findings were recorded.\n\n");
    } else {
        for finding in &report.findings {
            output.push_str(&format!(
                "### [{}] {}\n\n{}\n\n- Severity: {}\n- Confidence: {}%\n- Class: `{}`\n",
                markdown_text(&finding.category),
                markdown_text(&finding.code),
                markdown_text(&finding.message),
                finding.severity,
                finding.confidence,
                markdown_code(&finding.class)
            ));
            if !finding.evidence_ids.is_empty() {
                output.push_str(&format!(
                    "- Evidence: {}\n",
                    finding
                        .evidence_ids
                        .iter()
                        .map(|id| format!("`{}`", markdown_code(id)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if !finding.source_files.is_empty() {
                output.push_str(&format!(
                    "- Sources: {}\n",
                    finding
                        .source_files
                        .iter()
                        .map(|file| format!("`{}`", markdown_code(file)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            output.push('\n');
        }
    }

    output.push_str("## Verification\n\n");
    output.push_str(&format!(
        "**Verdict:** `{}`\n\n",
        markdown_code(&report.verification.verdict)
    ));
    for reason in &report.verification.reasons {
        output.push_str(&format!("- {}\n", markdown_text(reason)));
    }
    output.push('\n');

    output.push_str("## Baseline\n\n");
    output.push_str(&format!(
        "- Comparable: **{}**\n- Changed: **{}**\n- Reason: {}\n",
        report.baseline.comparable,
        report
            .baseline
            .changed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".into()),
        markdown_text(&report.baseline.reason)
    ));
    if let Some(hash) = &report.baseline.baseline_hash {
        output.push_str(&format!("- Baseline hash: `{}`\n", markdown_code(hash)));
    }
    output.push('\n');

    output.push_str("## Git\n\n");
    if report.git.available {
        output.push_str(&format!(
            "- Revision: `{}`\n- Branch: `{}`\n- Dirty: **{}**\n",
            markdown_code(report.git.revision.as_deref().unwrap_or("unknown")),
            markdown_code(report.git.branch.as_deref().unwrap_or("detached")),
            report.git.dirty.unwrap_or(false)
        ));
        if !report.git.changed_files.is_empty() {
            output.push_str("- Changed files:\n");
            for file in &report.git.changed_files {
                output.push_str(&format!("  - `{}`\n", markdown_code(file)));
            }
        }
    } else {
        output.push_str(&format!(
            "{}\n",
            markdown_text(report.git.reason.as_deref().unwrap_or("git unavailable"))
        ));
    }
    output.push('\n');

    if !report.incomplete_reasons.is_empty() {
        output.push_str("## Incomplete / inconclusive\n\n");
        for reason in &report.incomplete_reasons {
            output.push_str(&format!("- {}\n", markdown_text(reason)));
        }
    }
    output
}

pub fn render_wave8_html(report: &Wave8Report) -> String {
    let report = report.sanitized();
    let findings = report
        .findings
        .iter()
        .map(|finding| {
            format!(
                "<article><div class=\"meta\">{} · severity {} · {}% · {}</div><h3>{}</h3><p>{}</p></article>",
                html_escape(&finding.category),
                finding.severity,
                finding.confidence,
                html_escape(&finding.class),
                html_escape(&finding.code),
                html_escape(&finding.message)
            )
        })
        .collect::<Vec<_>>()
        .join("");

    let incomplete = report
        .incomplete_reasons
        .iter()
        .map(|reason| format!("<li>{}</li>", html_escape(reason)))
        .collect::<Vec<_>>()
        .join("");

    let git = if report.git.available {
        format!(
            "<p><code>{}</code> · <code>{}</code> · dirty={}</p>",
            html_escape(report.git.revision.as_deref().unwrap_or("unknown")),
            html_escape(report.git.branch.as_deref().unwrap_or("detached")),
            report.git.dirty.unwrap_or(false)
        )
    } else {
        format!(
            "<p>{}</p>",
            html_escape(report.git.reason.as_deref().unwrap_or("git unavailable"))
        )
    };

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>{}</title><style>body{{font:14px system-ui;max-width:980px;margin:40px auto;padding:0 24px;background:#0b0e13;color:#e8edf3}}article{{border:1px solid #26303d;border-radius:10px;padding:16px;margin:12px 0;background:#11161e}}.meta{{font:11px ui-monospace;color:#8290a4}}h1,h2,h3{{letter-spacing:-.02em}}code{{color:#9fc7ee}}.summary{{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:8px}}.card{{border:1px solid #26303d;border-radius:8px;padding:12px;background:#11161e}}</style></head><body><h1>{}</h1><p><code>{}</code> · <code>{}</code></p><div class="summary"><div class="card">Status<br><strong>{:?}</strong></div><div class="card">Deterministic<br><strong>{}</strong></div><div class="card">Heuristic<br><strong>{}</strong></div><div class="card">Subjective<br><strong>{}</strong></div></div><h2>Findings</h2>{}<h2>Verification</h2><p><code>{}</code></p><h2>Baseline</h2><p>{}</p><h2>Git</h2>{}<h2>Incomplete / inconclusive</h2><ul>{}</ul></body></html>"#,
        html_escape(&report.title),
        html_escape(&report.title),
        html_escape(&report.identity.project),
        html_escape(&report.identity.route),
        report.status,
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective,
        findings,
        html_escape(&report.verification.verdict),
        html_escape(&report.baseline.reason),
        git,
        incomplete
    )
}

fn sanitize_json_value(value: &mut Value, depth: usize) {
    if depth > 8 {
        *value = Value::String("<truncated>".into());
        return;
    }
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if sensitive_key(&key) {
                    map.insert(key, Value::String("<redacted>".into()));
                    continue;
                }
                if let Some(value) = map.get_mut(&key) {
                    sanitize_json_value(value, depth + 1);
                }
            }
        }
        Value::Array(values) => {
            values.truncate(MAX_IDS);
            for value in values {
                sanitize_json_value(value, depth + 1);
            }
        }
        Value::String(value) => {
            *value = bounded_text(value);
        }
        _ => {}
    }
}

fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [
        "token",
        "cookie",
        "password",
        "secret",
        "authorization",
        "input_value",
        "raw_value",
        "control_path",
        "absolute_path",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn bounded_text(input: &str) -> String {
    let filtered = input
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect::<String>();
    truncate_utf8(filtered.trim(), MAX_TEXT_BYTES)
}

fn bounded_route(input: &str) -> String {
    let text = bounded_text(input);
    let no_fragment = text.split('#').next().unwrap_or(&text);
    if let Some((path, _query)) = no_fragment.split_once('?') {
        path.to_owned()
    } else {
        no_fragment.to_owned()
    }
}

fn bounded_identifier(input: &str) -> String {
    truncate_utf8(
        &input
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ':' | '_' | '-' | '.' | '/'))
            .collect::<String>(),
        512,
    )
}

fn bounded_ids(values: &[String]) -> Vec<String> {
    let mut output = values
        .iter()
        .take(MAX_IDS)
        .map(|value| bounded_identifier(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    output.sort();
    output.dedup();
    output
}

fn bounded_texts(values: &[String], max: usize) -> Vec<String> {
    values.iter().take(max).map(|value| bounded_text(value)).collect()
}

fn bounded_relative_files(values: &[String]) -> Vec<String> {
    let mut output = BTreeSet::new();
    for value in values.iter().take(MAX_FILES) {
        let value = value.replace('\\', "/");
        if value.is_empty()
            || value.len() > 512
            || Path::new(&value).is_absolute()
            || looks_like_windows_absolute(&value)
            || value.split('/').any(|part| part == "..")
        {
            continue;
        }
        output.insert(value);
    }
    output.into_iter().collect()
}

fn looks_like_windows_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn markdown_text(input: &str) -> String {
    bounded_text(input)
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('#', "\\#")
}

fn markdown_code(input: &str) -> String {
    bounded_text(input).replace('`', "'").replace('\n', " ")
}

fn html_escape(input: &str) -> String {
    bounded_text(input)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave8_report() -> Wave8Report {
        Wave8Report {
            schema_version: WAVE8_REPORT_SCHEMA_VERSION,
            title: "Wave 8 report".into(),
            generated_at: "2026-09-21T00:00:00Z".into(),
            identity: Wave8Identity {
                project: "demo".into(),
                project_identity_hash: "sha256:project".into(),
                session_id: "session".into(),
                revision: Some("abc".into()),
                route: "/settings?token=secret#fragment".into(),
                viewport: Some((1280, 720)),
            },
            state_identity: "sha256:state".into(),
            diagnostics: Wave8DiagnosticCounts {
                deterministic: 1,
                heuristic: 1,
                subjective: 0,
            },
            findings: vec![Wave8Finding {
                category: "layout".into(),
                code: "overflow".into(),
                message: "<script>alert(1)</script>".into(),
                severity: 3,
                confidence: 100,
                class: "deterministic".into(),
                evidence_ids: vec!["ev_a".into()],
                source_files: vec!["src/app.rs".into()],
            }],
            verification: Wave8VerificationSummary {
                verdict: "pass".into(),
                reasons: vec!["fresh".into()],
                required_evidence_classes: vec!["semantic".into()],
                fresh_evidence_classes: vec!["semantic".into()],
            },
            baseline: Wave8BaselineComparison {
                baseline_hash: Some("sha256:baseline".into()),
                comparable: true,
                changed: Some(false),
                reason: "same state".into(),
            },
            evidence: Wave8EvidenceSummary {
                classes: vec!["semantic".into()],
                evidence_ids: vec!["ev_a".into()],
                evidence_hashes: vec!["sha256:ev".into()],
                proof_hashes: vec!["sha256:proof".into()],
            },
            git: Wave8GitAnnotation {
                available: true,
                revision: Some("abc".into()),
                branch: Some("main".into()),
                dirty: Some(false),
                changed_files: vec!["src/app.rs".into()],
                relevant_source_files: vec!["src/app.rs".into()],
                reason: None,
            },
            incomplete_reasons: Vec::new(),
            status: Wave8ReportStatus::Passed,
        }
    }

    #[test]
    fn html_escapes_project_name() {
        let report = LocalViewReport {
            title: "Audit".into(),
            generated_at: "now".into(),
            project: "<demo>".into(),
            route: "/".into(),
            viewport: None,
            diagnostics: DiagnosticReport::default(),
            metadata: BTreeMap::new(),
        };
        assert!(render_html(&report).contains("&lt;demo&gt;"));
    }

    #[test]
    fn wave8_html_escapes_all_project_content() {
        let bundle = produce_wave8_report_bundle(&wave8_report()).expect("bundle");
        assert!(!bundle.html.contains("<script>"));
        assert!(bundle.html.contains("&lt;script&gt;"));
    }

    #[test]
    fn wave8_markdown_escapes_project_content() {
        let mut report = wave8_report();
        report.findings[0].message = "# injected [link](bad)".into();
        let bundle = produce_wave8_report_bundle(&report).expect("bundle");
        assert!(bundle.markdown.contains("\\# injected \\[link\\](bad)"));
    }

    #[test]
    fn route_query_and_fragment_are_not_reported() {
        let bundle = produce_wave8_report_bundle(&wave8_report()).expect("bundle");
        assert!(bundle.json.contains("\\"route\\": \\"/settings\\""));
        assert!(!bundle.json.contains("secret"));
        assert!(!bundle.json.contains("fragment"));
    }

    #[test]
    fn absolute_source_paths_are_dropped() {
        let mut report = wave8_report();
        report.findings[0].source_files = vec![
            "/home/user/secret.rs".into(),
            "C:\\secret\\file.rs".into(),
            "src/safe.rs".into(),
        ];
        let sanitized = report.sanitized();
        assert_eq!(sanitized.findings[0].source_files, vec!["src/safe.rs"]);
    }

    #[test]
    fn canonical_report_hash_is_stable_for_same_sanitized_report() {
        let first = wave8_report();
        let mut second = first.clone();
        second.identity.route = "/settings?another=secret".into();
        assert_eq!(
            canonical_wave8_report_hash(&first),
            canonical_wave8_report_hash(&second)
        );
    }
}
