use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use localview_validation_lab::{LabMetricKind, LabSeedIdentity};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const PLATFORM_PROFILE: &str = "windows-uia-hosted-r1";
pub const COMPARISON_PROFILE: &str = "real-provider-exact-r1";
pub const RANDOM_SOURCE_PROFILE: &str = "deterministic-hosted-seed-protocol";
pub const CAMPAIGN_START_SEQUENCE: u64 = 100;

pub const REQUIRED_CASES: [&str; 11] = [
    "W01-missing-uia-property-event",
    "W02-recreated-uia-element",
    "W03-virtualized-item-realization",
    "W04-unsupported-invoke-pattern",
    "W05-windows-uia-provider-hang",
    "W06-windows-uia-provider-reacquire",
    "W07-foreground-stolen-before-input",
    "W08-partial-input-dispatch",
    "W09-user-held-modifier-interference",
    "W11-modal-before-dispatch",
    "W12-target-restart-after-authorization",
];

pub fn required_env(name: &'static str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be bound by the L7 CI gate"))
}

fn environment_value(name: &'static str) -> String {
    std::env::var(name).unwrap_or_else(|_| format!("unknown:{name}-not-exposed"))
}

pub fn executable_digest(env_name: &'static str) -> String {
    let path = required_env(env_name);
    let bytes = fs::read(&path)
        .unwrap_or_else(|error| panic!("read exact executable from {env_name}={path}: {error}"));
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

pub fn artifact_dir() -> PathBuf {
    let path = PathBuf::from(required_env("LOCALVIEW_L7_ARTIFACT_DIR"));
    fs::create_dir_all(&path).expect("create bounded L7 artifact directory");
    path
}

pub fn write_value(path: &Path, value: &Value) {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize bounded L7 JSON artifact"),
    )
    .expect("persist bounded L7 JSON artifact");
}

pub fn environment_manifest(
    candidate_sha: &str,
    classic_seed_digest: &str,
    edge_seed_digest: &str,
) -> Value {
    json!({
        "windows_build": environment_value("LOCALVIEW_WINDOWS_BUILD"),
        "architecture": std::env::var("RUNNER_ARCH").unwrap_or_else(|_| std::env::consts::ARCH.to_owned()),
        "localview_candidate_sha": candidate_sha,
        "provider_profile_revision": PLATFORM_PROFILE,
        "display_topology": environment_value("LOCALVIEW_DISPLAY_TOPOLOGY"),
        "dpi_scale": environment_value("LOCALVIEW_DPI_SCALE"),
        "locale_input_method": environment_value("LOCALVIEW_LOCALE_INPUT_METHOD"),
        "permission_state": environment_value("LOCALVIEW_PERMISSION_STATE"),
        "classic_seed_executable_digest": classic_seed_digest,
        "edge_seed_executable_digest": edge_seed_digest,
        "required_cases": REQUIRED_CASES,
        "explicitly_unmeasured_case": "W10-mixed-dpi",
        "w08_partial_evidence_source": "deterministic-wrapper-through-production-boundary",
        "w08_natural_windows_partial_observed": false,
        "w11_evidence_requirement": "exact-owned-modal-hwnd-plus-zero-effect",
        "w12_evidence_requirement": "A-B-lineage-plus-stale-rejection-plus-zero-B-effect"
    })
}

pub fn seed_identities() -> Vec<LabSeedIdentity> {
    let prediction_revisions = [
        "w01-missing-property-event-r1",
        "w02-recreated-element-r1",
        "w03-virtualized-realization-r1",
        "w04-unsupported-invoke-r1",
        "w05-provider-hang-r1",
        "w06-provider-reacquire-r1",
        "w07-foreground-stolen-r1",
        "w08-partial-input-dispatch-r1",
        "w09-modifier-interference-r1",
        "w11-modal-before-dispatch-r1",
        "w12-target-restart-after-authorization-r1",
    ];
    let oracle_revisions = [
        "independent-seed-pipe-r1",
        "independent-seed-pipe-r1",
        "independent-wpf-oracle-r1",
        "independent-seed-pipe-r1",
        "independent-wpf-oracle-r1",
        "independent-seed-pipe-r1",
        "independent-wpf-input-oracle-r1",
        "deterministic-partial-wrapper-plus-independent-wpf-full-smoke-r1",
        "independent-wpf-input-oracle-r1",
        "independent-wpf-modal-oracle-r1",
        "independent-process-lifecycle-plus-wpf-oracle-r1",
    ];

    REQUIRED_CASES
        .iter()
        .zip(prediction_revisions)
        .zip(oracle_revisions)
        .map(|((seed_id, prediction_revision), oracle_revision)| LabSeedIdentity {
            seed_id: (*seed_id).into(),
            prediction_revision: prediction_revision.into(),
            oracle_revision: oracle_revision.into(),
        })
        .collect()
}

pub fn expected_distinctions() -> BTreeSet<String> {
    BTreeSet::from([
        "event continuity != current snapshot completeness".into(),
        "element incarnation != provider incarnation".into(),
        "provider realization receipt != fresh action authority".into(),
        "unsupported semantic pattern != successful dispatch".into(),
        "provider timeout != reusable worker authority".into(),
        "provider reacquire invalidates old authority".into(),
        "preflight foreground != final input-boundary foreground".into(),
        "partial platform insertion != retry authority".into(),
        "user-held modifier != LocalView normalization authority".into(),
        "clean preflight != authority through a newly opened owned modal".into(),
        "logical target identity != reusable authority across process restart".into(),
    ])
}

pub fn assumptions() -> BTreeSet<String> {
    BTreeSet::from([
        "hosted Windows runner exposes real Win32 UI Automation".into(),
        "hosted Windows runner supports deterministic WPF UI Automation".into(),
        "production LocalView never reads either seed oracle channel".into(),
        "W10 remains explicitly unmeasured in this campaign".into(),
    ])
}

pub fn declared_metrics() -> BTreeSet<LabMetricKind> {
    BTreeSet::from([
        LabMetricKind::Rpomr,
        LabMetricKind::Eoffr,
        LabMetricKind::Rmr,
        LabMetricKind::Piaer,
        LabMetricKind::Scar,
        LabMetricKind::Cbfr,
    ])
}
