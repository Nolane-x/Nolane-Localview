#![cfg(windows)]

use std::{fs, path::PathBuf};

use serde_json::{Value, json};

use super::verified_input_seed::{EdgeSeedProcess, truth_bool, truth_u64};

const W10_CASE_ID: &str = "W10-mixed-dpi-window-movement";

#[derive(Debug)]
pub struct W10CapabilityRecord {
    pub case_id: &'static str,
    pub candidate_sha: String,
    pub monitor_count: u64,
    pub effective_dpi_values: Vec<u64>,
    pub mixed_dpi_capable: bool,
    pub measured: bool,
    pub reason: Option<String>,
    pub mints_real_provider_integration_pass: bool,
}

fn effective_dpi_values(value: &Value) -> Vec<u64> {
    let mut values = value
        .get("effective_dpi_values")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!("W10 geometry oracle must expose effective_dpi_values: {value}")
        })
        .iter()
        .map(|entry| {
            entry.as_u64().unwrap_or_else(|| {
                panic!("W10 effective DPI values must be unsigned integers: {value}")
            })
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

pub fn probe_and_write() -> W10CapabilityRecord {
    let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
        .expect("LOCALVIEW_CANDIDATE_SHA must bind W10 capability evidence to the exact candidate");
    assert!(
        !candidate_sha.trim().is_empty(),
        "LOCALVIEW_CANDIDATE_SHA must not be empty"
    );
    let artifact_dir = PathBuf::from(
        std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR")
            .expect("LOCALVIEW_L7_ARTIFACT_DIR must be set for W10 capability evidence"),
    );

    let mut seed = EdgeSeedProcess::spawn();
    let _fixture = seed.prepare_input_target();
    let truth = seed.command(json!({ "command": "get_geometry_state" }));
    assert_eq!(
        truth.get("ok").and_then(Value::as_bool),
        Some(true),
        "W10 capability probe requires an independent live geometry oracle: {truth}"
    );

    let monitor_count = truth_u64(&truth, "monitor_count");
    assert!(monitor_count >= 1, "W10 capability probe requires at least one attached display");
    let dpi_values = effective_dpi_values(&truth);
    assert!(
        !dpi_values.is_empty(),
        "W10 capability probe must record at least one effective DPI"
    );
    assert!(
        dpi_values.iter().all(|dpi| *dpi > 0),
        "W10 capability probe must not record zero effective DPI: {dpi_values:?}"
    );
    assert_eq!(
        truth_u64(&truth, "distinct_dpi_count"),
        u64::try_from(dpi_values.len()).expect("effective DPI set length must fit u64"),
        "W10 geometry oracle distinct_dpi_count must equal the persisted effective-DPI set"
    );

    let mixed_dpi_capable = truth_bool(&truth, "mixed_dpi_capable");
    assert_eq!(
        mixed_dpi_capable,
        monitor_count >= 2 && dpi_values.len() >= 2,
        "W10 mixed-DPI capability must be derived from real monitor count and distinct effective DPI"
    );

    // This permanent hosted probe records environment capability only. It never
    // executes the cross-monitor production/UIA comparison and therefore must
    // never mint L7 W10 closure, even if a hosted image happens to expose a
    // mixed-DPI topology in the future.
    let reason = if mixed_dpi_capable {
        "capability_only_probe_not_physical_w10_closure_authority"
    } else {
        "mixed_dpi_precondition_unmet"
    }
    .to_string();

    let record = W10CapabilityRecord {
        case_id: W10_CASE_ID,
        candidate_sha,
        monitor_count,
        effective_dpi_values: dpi_values,
        mixed_dpi_capable,
        measured: false,
        reason: Some(reason),
        mints_real_provider_integration_pass: false,
    };

    fs::create_dir_all(&artifact_dir).expect("create W10 capability artifact directory");
    let artifact = json!({
        "case_id": record.case_id,
        "candidate_sha": record.candidate_sha,
        "monitor_count": record.monitor_count,
        "effective_dpi_values": record.effective_dpi_values,
        "mixed_dpi_capable": record.mixed_dpi_capable,
        "measured": record.measured,
        "reason": record.reason,
        "mints_real_provider_integration_pass": record.mints_real_provider_integration_pass,
    });
    fs::write(
        artifact_dir.join("W10-CAPABILITY.json"),
        serde_json::to_vec_pretty(&artifact).expect("serialize W10 capability artifact"),
    )
    .expect("write W10 capability artifact");

    seed.shutdown();
    record
}
