#![forbid(unsafe_code)]

use localview_console::{ConsoleEntry, ConsoleGroup, ConsoleLevel};
use localview_live_bridge::{ObserverEvent, ObserverEventKind};
use localview_network::{NetworkFinding, NetworkPolicy, RequestRecord};
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
}

pub fn analyze_live(events: &[ObserverEvent]) -> LiveAnalysis {
    let mut network_records = Vec::new();
    let mut console_entries = Vec::new();
    let mut performance = PerformanceSample::default();
    let mut counts = LiveEventCounts { total: events.len(), ..Default::default() };

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

fn network_record(event: &ObserverEvent) -> RequestRecord {
    let payload = &event.payload;
    RequestRecord {
        id: format!("observer:{}", event.seq),
        method: text(payload, "method").unwrap_or("GET").to_owned(),
        url: text(payload, "url").unwrap_or("unknown").to_owned(),
        status: payload.get("status").and_then(|value| value.as_u64()).and_then(|value| u16::try_from(value).ok()),
        duration_ms: payload.get("duration").and_then(|value| value.as_f64()).unwrap_or(0.0).max(0.0).round() as u64,
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
        match text(payload, "level").unwrap_or("info").to_ascii_lowercase().as_str() {
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
            let duration = payload.get("duration").and_then(|value| value.as_f64()).unwrap_or(0.0).max(0.0).round() as u64;
            sample.long_tasks_ms.push(duration);
        }
        Some("layout_shift") => {
            let value = payload.get("value").and_then(|value| value.as_f64()).unwrap_or(0.0).max(0.0);
            sample.cumulative_layout_shift = Some(sample.cumulative_layout_shift.unwrap_or(0.0) + value);
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
        ObserverEvent { seq, captured_at: Utc::now(), kind, reference: None, route: Some("/".into()), payload }
    }

    #[test]
    fn turns_live_metadata_into_deterministic_analyzer_inputs() {
        let report = analyze_live(&[
            event(1, ObserverEventKind::Network, json!({"method":"GET","url":"http://localhost/api","status":500,"duration":25.0,"transport":"fetch"})),
            event(2, ObserverEventKind::Console, json!({"level":"warn","message":"React warning"})),
            event(3, ObserverEventKind::Performance, json!({"type":"long_task","duration":88.0})),
        ]);
        assert_eq!(report.counts.total, 3);
        assert_eq!(report.network.len(), 1);
        assert_eq!(report.console.len(), 1);
        assert_eq!(report.performance.len(), 1);
    }

    #[test]
    fn repeated_console_messages_are_grouped() {
        let report = analyze_live(&[
            event(1, ObserverEventKind::Console, json!({"level":"error","message":"boom"})),
            event(2, ObserverEventKind::Console, json!({"level":"error","message":"boom"})),
        ]);
        assert_eq!(report.console[0].count, 2);
    }
}
