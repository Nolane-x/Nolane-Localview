#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use localview_protocol::{ElementRef, Rect, SourceLocation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity { Info, Warning, Error }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualityFinding {
    pub code: String,
    pub severity: Severity,
    pub reference: Option<ElementRef>,
    pub message: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    HttpStatus(u16),
    TimeoutMs(u64),
    Offline,
    Abort,
    FixedBody { status: u16, body: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MockRule {
    pub id: String,
    pub method: Option<String>,
    pub url_contains: String,
    pub mode: FailureMode,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestDescriptor { pub method: String, pub url: String }

pub fn matching_failure<'a>(request: &RequestDescriptor, rules: &'a [MockRule]) -> Option<&'a MockRule> {
    rules.iter().find(|rule| {
        rule.enabled
            && request.url.contains(&rule.url_contains)
            && rule.method.as_ref().is_none_or(|method| method.eq_ignore_ascii_case(&request.method))
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyboardNode {
    pub reference: ElementRef,
    pub focusable: bool,
    pub visible: bool,
    pub tabindex: i32,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyboardJourney {
    pub order: Vec<ElementRef>,
    pub unreachable: Vec<ElementRef>,
    pub suspicious_positive_tabindex: Vec<ElementRef>,
}

pub fn analyze_keyboard(nodes: &[KeyboardNode]) -> KeyboardJourney {
    let mut positive = nodes.iter().filter(|node| node.focusable && node.visible && node.tabindex > 0).collect::<Vec<_>>();
    positive.sort_by_key(|node| node.tabindex);
    let natural = nodes.iter().filter(|node| node.focusable && node.visible && node.tabindex == 0);
    let order = positive.iter().map(|node| node.reference.clone()).chain(natural.map(|node| node.reference.clone())).collect();
    let unreachable = nodes.iter().filter(|node| node.focusable && (!node.visible || node.tabindex < 0)).map(|node| node.reference.clone()).collect();
    let suspicious_positive_tabindex = positive.into_iter().map(|node| node.reference.clone()).collect();
    KeyboardJourney { order, unreachable, suspicious_positive_tabindex }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PointerTarget {
    pub reference: ElementRef,
    pub rect: Rect,
    pub interactive: bool,
    pub pointer_handler: bool,
    pub obscured_by: Vec<ElementRef>,
    pub feedback_ms: Option<u64>,
}

pub fn pointer_findings(targets: &[PointerTarget]) -> Vec<QualityFinding> {
    let mut findings = Vec::new();
    for target in targets {
        if target.interactive && (target.rect.width < 44.0 || target.rect.height < 44.0) {
            findings.push(finding("touch_target_small", Severity::Warning, &target.reference, "Interactive target is smaller than the 44×44 logical-pixel comfort target", 0.92));
        }
        if target.interactive && !target.pointer_handler {
            findings.push(finding("dead_click_candidate", Severity::Error, &target.reference, "Interactive-looking target has no pointer handler evidence", 0.88));
        }
        if !target.obscured_by.is_empty() {
            findings.push(finding("occluded_interaction", Severity::Error, &target.reference, &format!("Target is obscured by {} element(s)", target.obscured_by.len()), 0.96));
        }
        if target.feedback_ms.is_some_and(|latency| latency > 250) {
            findings.push(finding("feedback_latency", Severity::Warning, &target.reference, "Visible interaction feedback exceeded 250 ms", 0.85));
        }
    }
    findings
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentStressCase {
    pub id: String,
    pub text: String,
    pub purpose: String,
}

pub fn default_content_stress_cases(seed: &str) -> Vec<ContentStressCase> {
    vec![
        ContentStressCase { id: "empty".into(), text: String::new(), purpose: "empty-state resilience".into() },
        ContentStressCase { id: "long".into(), text: seed.repeat(12), purpose: "overflow and wrapping".into() },
        ContentStressCase { id: "unbroken".into(), text: "W".repeat(160), purpose: "unbroken token overflow".into() },
        ContentStressCase { id: "unicode".into(), text: format!("{seed} — 日本語 العربية 😀 é"), purpose: "unicode shaping".into() },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocaleCase { pub locale: String, pub rtl: bool, pub pseudo: bool }

pub fn default_locale_sweep() -> Vec<LocaleCase> {
    vec![
        LocaleCase { locale: "en-US".into(), rtl: false, pseudo: false },
        LocaleCase { locale: "de-DE".into(), rtl: false, pseudo: false },
        LocaleCase { locale: "ja-JP".into(), rtl: false, pseudo: false },
        LocaleCase { locale: "ar".into(), rtl: true, pseudo: false },
        LocaleCase { locale: "en-XA".into(), rtl: false, pseudo: true },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionSample {
    pub reference: ElementRef,
    pub duration_ms: u64,
    pub repeats_forever: bool,
    pub honors_reduced_motion: bool,
    pub layout_affecting: bool,
}

pub fn motion_findings(samples: &[MotionSample]) -> Vec<QualityFinding> {
    let mut findings = Vec::new();
    for sample in samples {
        if sample.duration_ms > 700 && sample.layout_affecting {
            findings.push(finding("motion_slow_layout", Severity::Warning, &sample.reference, "Long motion changes layout and may make the interface feel unstable", 0.82));
        }
        if sample.repeats_forever && !sample.honors_reduced_motion {
            findings.push(finding("reduced_motion_missing", Severity::Error, &sample.reference, "Infinite motion has no prefers-reduced-motion evidence", 0.94));
        }
    }
    findings
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HierarchyNode {
    pub reference: ElementRef,
    pub area: f64,
    pub font_size: f64,
    pub font_weight: u16,
    pub contrast: f64,
    pub interactive: bool,
}

pub fn hierarchy_findings(nodes: &[HierarchyNode]) -> Vec<QualityFinding> {
    if nodes.is_empty() { return Vec::new(); }
    let max_font = nodes.iter().map(|node| node.font_size).fold(0.0_f64, f64::max);
    let mut findings = Vec::new();
    for node in nodes {
        if node.interactive && node.contrast < 3.0 {
            findings.push(finding("weak_interactive_contrast", Severity::Warning, &node.reference, "Interactive control has weak visual contrast", 0.80));
        }
        if max_font >= 24.0 && node.font_size >= max_font * 0.95 && node.area < 400.0 {
            findings.push(finding("hierarchy_fragment", Severity::Info, &node.reference, "Largest typography is attached to a very small visual region", 0.64));
        }
    }
    findings
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScrollMetrics {
    pub viewport_height: f64,
    pub content_height: f64,
    pub max_scroll_y: f64,
    pub sticky_overlaps: Vec<ElementRef>,
    pub nested_scroll_regions: usize,
}

pub fn scroll_findings(metrics: &ScrollMetrics) -> Vec<QualityFinding> {
    let mut findings = Vec::new();
    if metrics.content_height > metrics.viewport_height && metrics.max_scroll_y <= 0.0 {
        findings.push(global_finding("scroll_blocked", Severity::Error, "Content exceeds viewport but no scroll range is available", 0.98));
    }
    if !metrics.sticky_overlaps.is_empty() {
        findings.push(global_finding("sticky_occlusion", Severity::Warning, &format!("{} sticky element(s) may cover content while scrolling", metrics.sticky_overlaps.len()), 0.87));
    }
    if metrics.nested_scroll_regions > 3 {
        findings.push(global_finding("nested_scroll_complexity", Severity::Info, "Many nested scroll regions increase interaction complexity", 0.70));
    }
    findings
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignSnapshot {
    pub tokens: BTreeMap<String, String>,
    pub boxes: BTreeMap<ElementRef, Rect>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignRegression {
    pub token_changes: BTreeMap<String, (Option<String>, Option<String>)>,
    pub moved_refs: Vec<ElementRef>,
}

pub fn diff_design(before: &DesignSnapshot, after: &DesignSnapshot, geometry_tolerance: f64) -> DesignRegression {
    let keys = before.tokens.keys().chain(after.tokens.keys()).cloned().collect::<BTreeSet<_>>();
    let token_changes = keys.into_iter().filter_map(|key| {
        let old = before.tokens.get(&key).cloned();
        let new = after.tokens.get(&key).cloned();
        (old != new).then_some((key, (old, new)))
    }).collect();
    let moved_refs = before.boxes.iter().filter_map(|(reference, old)| {
        let new = after.boxes.get(reference)?;
        let delta = (old.x - new.x).abs() + (old.y - new.y).abs() + (old.width - new.width).abs() + (old.height - new.height).abs();
        (delta > geometry_tolerance).then(|| reference.clone())
    }).collect();
    DesignRegression { token_changes, moved_refs }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitContext {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub dirty: bool,
    pub changed_files: Vec<String>,
}

impl GitContext {
    pub fn session_suffix(&self) -> String {
        let branch = self.branch.as_deref().unwrap_or("detached");
        let head = self.head.as_deref().unwrap_or("unknown");
        let short = head.get(..head.len().min(8)).unwrap_or(head);
        format!("{branch}@{short}{}", if self.dirty { "+dirty" } else { "" })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IssueCandidate {
    pub reference: ElementRef,
    pub code: String,
    pub source: Option<SourceLocation>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FixLoopPlan {
    pub issue_code: String,
    pub reference: ElementRef,
    pub source: Option<SourceLocation>,
    pub verify_steps: Vec<String>,
}

pub fn build_fix_loop(issue: &IssueCandidate) -> FixLoopPlan {
    FixLoopPlan {
        issue_code: issue.code.clone(),
        reference: issue.reference.clone(),
        source: issue.source.clone(),
        verify_steps: vec![
            "capture baseline evidence".into(),
            "apply source edit".into(),
            "wait for HMR settle".into(),
            "re-run deterministic check".into(),
            "compare state/layout/visual delta".into(),
        ],
    }
}

fn finding(code: &str, severity: Severity, reference: &str, message: &str, confidence: f32) -> QualityFinding {
    QualityFinding { code: code.into(), severity, reference: Some(reference.into()), message: message.into(), confidence }
}

fn global_finding(code: &str, severity: Severity, message: &str, confidence: f32) -> QualityFinding {
    QualityFinding { code: code.into(), severity, reference: None, message: message.into(), confidence }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(width: f64, height: f64) -> Rect { Rect { x: 0.0, y: 0.0, width, height } }

    #[test]
    fn failure_injection_matches_method_and_url() {
        let rules = vec![MockRule { id: "login-500".into(), method: Some("POST".into()), url_contains: "/login".into(), mode: FailureMode::HttpStatus(500), enabled: true }];
        let request = RequestDescriptor { method: "post".into(), url: "http://localhost/api/login".into() };
        assert_eq!(matching_failure(&request, &rules).map(|rule| rule.id.as_str()), Some("login-500"));
    }

    #[test]
    fn pointer_analysis_finds_small_dead_and_occluded_target() {
        let findings = pointer_findings(&[PointerTarget { reference: "button:save".into(), rect: rect(24.0, 24.0), interactive: true, pointer_handler: false, obscured_by: vec!["dialog".into()], feedback_ms: Some(400) }]);
        assert_eq!(findings.len(), 4);
    }

    #[test]
    fn design_diff_reports_token_and_geometry_changes() {
        let before = DesignSnapshot { tokens: BTreeMap::from([("radius".into(), "8px".into())]), boxes: BTreeMap::from([("hero".into(), rect(100.0, 100.0))]) };
        let mut after = before.clone();
        after.tokens.insert("radius".into(), "12px".into());
        after.boxes.insert("hero".into(), Rect { x: 20.0, y: 0.0, width: 100.0, height: 100.0 });
        let diff = diff_design(&before, &after, 4.0);
        assert!(diff.token_changes.contains_key("radius"));
        assert_eq!(diff.moved_refs, vec!["hero"]);
    }
}
