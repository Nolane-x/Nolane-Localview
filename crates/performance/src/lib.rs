#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const PERFORMANCE_LITE_PACKET_VERSION: u8 = 1;
pub const DEFAULT_LONG_TASK_SAMPLE_BUDGET: u8 = 8;
pub const MAX_LONG_TASK_SAMPLE_BUDGET: u8 = 16;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceSample {
    pub render_ms: Option<u64>,
    pub long_tasks_ms: Vec<u64>,
    pub cumulative_layout_shift: Option<f64>,
    pub transferred_bytes: Option<u64>,
    pub heap_bytes: Option<u64>,
    pub hmr_settle_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerformanceFinding {
    pub code: String,
    pub severity: u8,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerformanceLiteBudget {
    pub max_long_task_samples: u8,
}

impl Default for PerformanceLiteBudget {
    fn default() -> Self {
        Self {
            max_long_task_samples: DEFAULT_LONG_TASK_SAMPLE_BUDGET,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PerformanceLitePacket {
    pub version: u8,
    pub long_task_count: u32,
    pub sampled_long_tasks_ms: Vec<u64>,
    pub omitted_long_task_samples: u32,
    pub total_long_task_ms: u64,
    pub max_long_task_ms: Option<u64>,
    pub cumulative_layout_shift: Option<f64>,
    pub applied_long_task_sample_budget: u8,
}

impl Default for PerformanceLitePacket {
    fn default() -> Self {
        Self {
            version: PERFORMANCE_LITE_PACKET_VERSION,
            long_task_count: 0,
            sampled_long_tasks_ms: Vec::new(),
            omitted_long_task_samples: 0,
            total_long_task_ms: 0,
            max_long_task_ms: None,
            cumulative_layout_shift: None,
            applied_long_task_sample_budget: DEFAULT_LONG_TASK_SAMPLE_BUDGET,
        }
    }
}

pub fn lite_packet(
    sample: &PerformanceSample,
    budget: PerformanceLiteBudget,
) -> PerformanceLitePacket {
    let applied_budget = budget
        .max_long_task_samples
        .min(MAX_LONG_TASK_SAMPLE_BUDGET);
    let sample_cap = usize::from(applied_budget);
    let mut sampled_long_tasks_ms = Vec::with_capacity(sample_cap);
    let mut long_task_count = 0_u32;
    let mut total_long_task_ms = 0_u64;
    let mut max_long_task_ms = None;

    for &duration in &sample.long_tasks_ms {
        long_task_count = long_task_count.saturating_add(1);
        total_long_task_ms = total_long_task_ms.saturating_add(duration);
        max_long_task_ms = Some(max_long_task_ms.map_or(duration, |current: u64| {
            current.max(duration)
        }));

        if sample_cap == 0 {
            continue;
        }

        let insert_at = sampled_long_tasks_ms
            .iter()
            .position(|existing| duration > *existing)
            .unwrap_or(sampled_long_tasks_ms.len());

        if insert_at < sample_cap {
            sampled_long_tasks_ms.insert(insert_at, duration);
            if sampled_long_tasks_ms.len() > sample_cap {
                sampled_long_tasks_ms.pop();
            }
        } else if sampled_long_tasks_ms.len() < sample_cap {
            sampled_long_tasks_ms.push(duration);
        }
    }

    let sampled_count = u32::try_from(sampled_long_tasks_ms.len()).unwrap_or(u32::MAX);
    let cumulative_layout_shift = sample
        .cumulative_layout_shift
        .filter(|value| value.is_finite() && *value >= 0.0);

    PerformanceLitePacket {
        version: PERFORMANCE_LITE_PACKET_VERSION,
        long_task_count,
        sampled_long_tasks_ms,
        omitted_long_task_samples: long_task_count.saturating_sub(sampled_count),
        total_long_task_ms,
        max_long_task_ms,
        cumulative_layout_shift,
        applied_long_task_sample_budget: applied_budget,
    }
}

pub fn analyze(sample: &PerformanceSample) -> Vec<PerformanceFinding> {
    let mut findings = Vec::new();

    for task_ms in &sample.long_tasks_ms {
        if *task_ms >= 50 {
            findings.push(PerformanceFinding {
                code: "long_task".into(),
                severity: 2,
                message: format!("Main-thread task lasted {task_ms}ms"),
            });
        }
    }

    if let Some(value) = sample
        .cumulative_layout_shift
        .filter(|value| value.is_finite() && *value > 0.25)
    {
        findings.push(PerformanceFinding {
            code: "layout_instability".into(),
            severity: 2,
            message: format!("CLS {value:.3}"),
        });
    }

    if let Some(hmr_settle_ms) = sample.hmr_settle_ms.filter(|value| *value > 2_000) {
        findings.push(PerformanceFinding {
            code: "slow_hmr".into(),
            severity: 1,
            message: format!("HMR settle {hmr_settle_ms}ms"),
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_with_tasks(tasks: impl IntoIterator<Item = u64>) -> PerformanceSample {
        PerformanceSample {
            long_tasks_ms: tasks.into_iter().collect(),
            cumulative_layout_shift: Some(0.3),
            ..Default::default()
        }
    }

    #[test]
    fn lite_packet_keeps_truthful_aggregates_while_sampling_longest_tasks() {
        let packet = lite_packet(
            &sample_with_tasks(50..=69),
            PerformanceLiteBudget::default(),
        );

        assert_eq!(packet.version, 1);
        assert_eq!(packet.long_task_count, 20);
        assert_eq!(packet.total_long_task_ms, 1_190);
        assert_eq!(packet.max_long_task_ms, Some(69));
        assert_eq!(
            packet.sampled_long_tasks_ms,
            vec![69, 68, 67, 66, 65, 64, 63, 62]
        );
        assert_eq!(packet.omitted_long_task_samples, 12);
        assert_eq!(packet.applied_long_task_sample_budget, 8);
        assert_eq!(packet.cumulative_layout_shift, Some(0.3));
    }

    #[test]
    fn lite_packet_hard_caps_caller_sample_budget() {
        let packet = lite_packet(
            &sample_with_tasks(1..=40),
            PerformanceLiteBudget {
                max_long_task_samples: u8::MAX,
            },
        );

        assert_eq!(
            packet.applied_long_task_sample_budget,
            MAX_LONG_TASK_SAMPLE_BUDGET
        );
        assert_eq!(
            packet.sampled_long_tasks_ms.len(),
            usize::from(MAX_LONG_TASK_SAMPLE_BUDGET)
        );
        assert_eq!(packet.sampled_long_tasks_ms[0], 40);
        assert_eq!(packet.sampled_long_tasks_ms.last().copied(), Some(25));
        assert_eq!(packet.omitted_long_task_samples, 24);
    }

    #[test]
    fn lite_packet_zero_budget_is_aggregate_only() {
        let packet = lite_packet(
            &sample_with_tasks([75, 125, 50]),
            PerformanceLiteBudget {
                max_long_task_samples: 0,
            },
        );

        assert!(packet.sampled_long_tasks_ms.is_empty());
        assert_eq!(packet.long_task_count, 3);
        assert_eq!(packet.total_long_task_ms, 250);
        assert_eq!(packet.max_long_task_ms, Some(125));
        assert_eq!(packet.omitted_long_task_samples, 3);
        assert_eq!(packet.applied_long_task_sample_budget, 0);
    }

    #[test]
    fn lite_packet_drops_invalid_layout_shift_values() {
        let negative = lite_packet(
            &PerformanceSample {
                cumulative_layout_shift: Some(-0.25),
                ..Default::default()
            },
            PerformanceLiteBudget::default(),
        );
        let non_finite = lite_packet(
            &PerformanceSample {
                cumulative_layout_shift: Some(f64::INFINITY),
                ..Default::default()
            },
            PerformanceLiteBudget::default(),
        );

        assert_eq!(negative.cumulative_layout_shift, None);
        assert_eq!(non_finite.cumulative_layout_shift, None);
    }

    #[test]
    fn analyzer_retains_existing_deterministic_findings() {
        let findings = analyze(&PerformanceSample {
            long_tasks_ms: vec![49, 50, 88],
            cumulative_layout_shift: Some(0.26),
            hmr_settle_ms: Some(2_001),
            ..Default::default()
        });

        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.code.as_str())
                .collect::<Vec<_>>(),
            vec!["long_task", "long_task", "layout_instability", "slow_hmr"]
        );
    }
}
