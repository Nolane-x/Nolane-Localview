#![forbid(unsafe_code)]

use localview_console::{ConsoleEntry, ConsoleGroup, ConsoleLevel};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_network::{NetworkFinding, NetworkIssueKind, NetworkPolicy, RequestRecord};
use localview_performance::{PerformanceFinding, PerformanceSample};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveAnalysis {
    pub network: Vec<NetworkFinding>,
    pub console: Vec<ConsoleGroup>,
    pub performance: Vec<PerformanceFinding>,
    pub counts: LiveEventCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LiveEventCounts {
    pub total: usize,
    pub dom: usize,
    pub console: usize,
    pub network: usize,
    pub runtime_errors: usize,
    pub performance: usize,
    pub semantic_snapshots: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingClass {
    Deterministic,
    Heuristic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosisFinding {
    pub category: String,
    pub code: String,
    pub message: String,
    pub severity: u8,
    pub confidence: u8,
    pub class: FindingClass,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveUncertaintyClass {
    Identity,
    Cause,
    Visual,
    State,
    Browser,
    Intent,
    Security,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveUncertainty {
    pub class: LiveUncertaintyClass,
    pub statement: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LiveDiagnosis {
    pub findings: Vec<DiagnosisFinding>,
    pub unknowns: Vec<LiveUncertainty>,
    pub recommended_actions: Vec<String>,
    pub analysis: LiveAnalysis,
}

pub fn analyze_live(events: &[ObserverEvent]) -> LiveAnalysis {
    let mut network_records = Vec::new();
    let mut console_entries = Vec::new();
    let mut performance = PerformanceSample::default();
    let mut counts = LiveEventCounts {
        total: events.len(),
        ..Default::default()
    };

    for event in events {
        match event.kind {
            ObserverEventKind::DomMutation | ObserverEventKind::Layout => counts.dom += 1,
            ObserverEventKind::Console => {
                counts.console += 1;
                console_entries.push(console_entry(event, false));
            }
            ObserverEventKind::RuntimeError => {
                counts.runtime_errors += 1;
                console_entries.push(console_entry(event, true));
            }
            ObserverEventKind::Network => {
                counts.network += 1;
                network_records.push(network_record(event));
            }
            ObserverEventKind::Performance => {
                counts.performance += 1;
                apply_performance(event, &mut performance);
            }
            ObserverEventKind::SemanticSnapshot => counts.semantic_snapshots += 1,
            _ => {}
        }
    }

    LiveAnalysis {
        network: localview_network::analyze(&network_records, &NetworkPolicy::default()),
        console: localview_console::group(&console_entries),
        performance: localview_performance::analyze(&performance),
        counts,
    }
}

pub fn diagnose_live(events: &[ObserverEvent]) -> LiveDiagnosis {
    let analysis = analyze_live(events);
    let mut findings = Vec::new();

    for finding in &analysis.network {
        let (severity, class) = match &finding.kind {
            NetworkIssueKind::Failed | NetworkIssueKind::Cors | NetworkIssueKind::MixedContent => {
                (3, FindingClass::Deterministic)
            }
            NetworkIssueKind::Slow | NetworkIssueKind::LargePayload => {
                (2, FindingClass::Deterministic)
            }
            NetworkIssueKind::Duplicate => (2, FindingClass::Heuristic),
        };
        findings.push(DiagnosisFinding {
            category: "network".into(),
            code: network_code(&finding.kind).into(),
            message: finding.message.clone(),
            severity,
            confidence: finding.confidence,
            class,
        });
    }

    for group in &analysis.console {
        findings.push(DiagnosisFinding {
            category: "console".into(),
            code: console_code(&group.level).into(),
            message: if group.count > 1 {
                format!("{} ({} occurrences)", group.message, group.count)
            } else {
                group.message.clone()
            },
            severity: match group.level {
                ConsoleLevel::Error => 3,
                ConsoleLevel::Warning => 2,
                ConsoleLevel::Info | ConsoleLevel::Debug => 1,
            },
            confidence: 100,
            class: FindingClass::Deterministic,
        });
    }

    for finding in &analysis.performance {
        findings.push(DiagnosisFinding {
            category: "performance".into(),
            code: finding.code.clone(),
            message: finding.message.clone(),
            severity: finding.severity,
            confidence: 100,
            class: FindingClass::Deterministic,
        });
    }

    findings.sort_by(|left, right| {
        right
            .severity
            .cmp(&left.severity)
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| left.code.cmp(&right.code))
    });

    let mut unknowns = Vec::new();
    let has_layout = events.iter().any(|event| event.kind == ObserverEventKind::Layout);
    if events.is_empty() {
        unknowns.push(LiveUncertainty {
            class: LiveUncertaintyClass::Identity,
            statement: "No live preview evidence is attached to this session yet".into(),
            reason: "The retained observer window is empty".into(),
        });
    }
    if analysis.counts.semantic_snapshots == 0 {
        unknowns.push(LiveUncertainty {
            class: LiveUncertaintyClass::State,
            statement: "Current semantic page state is unknown".into(),
            reason: "No semantic snapshot exists in the retained observer window".into(),
        });
    }
    if !has_layout {
        unknowns.push(LiveUncertainty {
            class: LiveUncertaintyClass::Visual,
            statement: "Current layout geometry has not been verified".into(),
            reason: "No layout evidence exists in the retained observer window".into(),
        });
    }
    if !findings.is_empty() {
        unknowns.push(LiveUncertainty {
            class: LiveUncertaintyClass::Cause,
            statement: "Observed failures are not yet proven root causes".into(),
            reason: "Live telemetry alone does not establish a source/causal chain".into(),
        });
    }

    let mut recommended_actions = Vec::new();
    if events.is_empty() {
        recommended_actions.push("Open the native preview so LocalView can attach its secure observer".into());
    }
    if analysis.counts.semantic_snapshots == 0 {
        recommended_actions.push("Queue a semantic snapshot before making state-dependent claims".into());
    }
    if !has_layout {
        recommended_actions.push("Run X-Ray/layout inspection before claiming a visual root cause".into());
    }
    if !analysis.network.is_empty() {
        recommended_actions.push("Trace failed or slow requests to their initiating interaction/source".into());
    }
    if !analysis.console.is_empty() {
        recommended_actions.push("Trace grouped console errors to source-map evidence".into());
    }
    if !analysis.performance.is_empty() {
        recommended_actions.push("Capture a targeted performance sample around the affected interaction".into());
    }
    if findings.is_empty() && events.len() > 0 {
        recommended_actions.push("No retained deterministic failure was found; expand evidence only if the issue still reproduces".into());
    }

    LiveDiagnosis {
        findings,
        unknowns,
        recommended_actions,
        analysis,
    }
}

fn network_code(kind: &NetworkIssueKind) -> &'static str {
    match kind {
        NetworkIssueKind::Failed => "network.failed",
        NetworkIssueKind::Slow => "network.slow",
        NetworkIssueKind::Duplicate => "network.duplicate",
        NetworkIssueKind::LargePayload => "network.large_payload",
        NetworkIssueKind::Cors => "network.cors",
        NetworkIssueKind::MixedContent => "network.mixed_content",
    }
}

fn console_code(level: &ConsoleLevel) -> &'static str {
    match level {
        ConsoleLevel::Debug => "console.debug",
        ConsoleLevel::Info => "console.info",
        ConsoleLevel::Warning => "console.warning",
        ConsoleLevel::Error => "console.error",
    }
}

fn network_record(event: &ObserverEvent) -> RequestRecord {
    let payload = &event.payload;
    RequestRecord {
        id: format!("observer:{}", event.seq),
        method: text(payload, "method").unwrap_or("GET").to_owned(),
        url: text(payload, "url").unwrap_or("unknown").to_owned(),
        status: payload
            .get("status")
            .and_then(|value| value.as_u64())
            .and_then(|value| u16::try_from(value).ok()),
        duration_ms: payload
            .get("duration")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0)
            .max(0.0)
            .round() as u64,
        encoded_bytes: None,
        from_cache: false,
        error: text(payload, "error").map(str::to_owned),
        initiator: text(payload, "transport").map(str::to_owned),
    }
}

fn console_entry(event: &ObserverEvent, runtime_error: bool) -> ConsoleEntry {
    let payload = &event.payload;
    let level = if runtime_error {
        ConsoleLevel::Error
    } else {
        match text(payload, "level")
            .unwrap_or("info")
            .to_ascii_lowercase()
            .as_str()
        {
            "debug" => ConsoleLevel::Debug,
            "warn" | "warning" => ConsoleLevel::Warning,
            "error" => ConsoleLevel::Error,
            _ => ConsoleLevel::Info,
        }
    };
    ConsoleEntry {
        level,
        message: text(payload, "message").unwrap_or("runtime event").to_owned(),
        stack: text(payload, "stack").map(str::to_owned),
        source: text(payload, "source").map(str::to_owned),
        action_ref: event.reference.clone(),
    }
}

fn apply_performance(event: &ObserverEvent, sample: &mut PerformanceSample) {
    let payload = &event.payload;
    match text(payload, "type") {
        Some("long_task") => {
            let duration = payload
                .get("duration")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0)
                .max(0.0)
                .round() as u64;
            sample.long_tasks_ms.push(duration);
        }
        Some("layout_shift") => {
            let value = payload
                .get("value")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0)
                .max(0.0);
            sample.cumulative_layout_shift =
                Some(sample.cumulative_layout_shift.unwrap_or(0.0) + value);
        }
        _ => {}
    }
}

fn text<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use localview_live_bridge::{ObserverEvent, ObserverEventKind};
    use serde_json::json;

    use super::*;

    fn event(seq: u64, kind: ObserverEventKind, payload: serde_json::Value) -> ObserverEvent {
        ObserverEvent {
            seq,
            captured_at: Utc::now(),
            kind,
            reference: None,
            route: Some("/".into()),
            payload,
        }
    }

    #[test]
    fn turns_live_metadata_into_deterministic_analyzer_inputs() {
        let report = analyze_live(&[
            event(
                1,
                ObserverEventKind::Network,
                json!({"method":"GET","url":"http://localhost/api","status":500,"duration":25.0,"transport":"fetch"}),
            ),
            event(
                2,
                ObserverEventKind::Console,
                json!({"level":"warn","message":"React warning"}),
            ),
            event(
                3,
                ObserverEventKind::Performance,
                json!({"type":"long_task","duration":88.0}),
            ),
        ]);
        assert_eq!(report.counts.total, 3);
        assert_eq!(report.network.len(), 1);
        assert_eq!(report.console.len(), 1);
        assert_eq!(report.performance.len(), 1);
    }

    #[test]
    fn repeated_console_messages_are_grouped() {
        let report = analyze_live(&[
            event(
                1,
                ObserverEventKind::Console,
                json!({"level":"error","message":"boom"}),
            ),
            event(
                2,
                ObserverEventKind::Console,
                json!({"level":"error","message":"boom"}),
            ),
        ]);
        assert_eq!(report.console[0].count, 2);
    }

    #[test]
    fn diagnosis_preserves_cause_uncertainty_until_causal_evidence_exists() {
        let report = diagnose_live(&[event(
            1,
            ObserverEventKind::Network,
            json!({"method":"GET","url":"http://localhost/api","status":500,"duration":10.0}),
        )]);
        assert!(!report.findings.is_empty());
        assert!(report
            .unknowns
            .iter()
            .any(|unknown| unknown.class == LiveUncertaintyClass::Cause));
    }

    #[test]
    fn empty_stream_recommends_attaching_observer_instead_of_inventing_findings() {
        let report = diagnose_live(&[]);
        assert!(report.findings.is_empty());
        assert!(report
            .unknowns
            .iter()
            .any(|unknown| unknown.class == LiveUncertaintyClass::Identity));
    }
}
