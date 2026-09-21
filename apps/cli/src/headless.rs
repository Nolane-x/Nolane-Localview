use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, ValueEnum};
use localview_artifacts::ArtifactStore;
use localview_attestation::{DigestAttestationPayload, digest_attestation};
use localview_content_addressed::{BaselineEnvelope, object_hash};
use localview_diagnostics::{DiagnosticClass, DiagnosticReport};
use localview_protocol::{PageSnapshot, Session, SessionId};
use localview_reports::{
    ArtifactReference, BaselineComparison, BaselineComparisonStatus, GitAnnotation, LocalViewReport,
    ReportStatus, render_html, render_json, render_markdown,
};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{process::Command, time::timeout};

pub const EXIT_PASS: i32 = 0;
pub const EXIT_HARD_FAILURE: i32 = 2;
pub const EXIT_INCONCLUSIVE: i32 = 3;
pub const EXIT_INFRASTRUCTURE: i32 = 4;

const MAX_EVIDENCE: usize = 128;
const MAX_CHANGED_FILES: usize = 128;
const MAX_RELEVANT_FILES: usize = 64;
const MAX_DIAGNOSTIC_TEXT: usize = 512;
const MAX_FIXTURE_BYTES: u64 = 256 * 1024;
const MAX_BASELINE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_ARTIFACT_BUDGET_MIB: u64 = 64;
const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 30_000;
const MAX_COMMAND_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Clone, Args)]
pub struct HeadlessArgs {
    #[arg(long)]
    pub session: Option<SessionId>,
    #[arg(long, value_enum, default_value_t = HeadlessAnalysis::Diagnose)]
    pub analysis: HeadlessAnalysis,
    #[arg(long)]
    pub fixture: Option<PathBuf>,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    #[arg(long)]
    pub visual: bool,
    #[arg(long)]
    pub chromium: bool,
    #[arg(long)]
    pub update_baseline: bool,
    #[arg(long)]
    pub require_baseline_match: bool,
    #[arg(long)]
    pub require_verification_pass: bool,
    #[arg(long)]
    pub fail_on_heuristic: bool,
    #[arg(long, default_value_t = 3)]
    pub deterministic_severity: u8,
    #[arg(long, default_value_t = 0.01)]
    pub visual_max_changed_ratio: f64,
    #[arg(long, default_value_t = DEFAULT_ARTIFACT_BUDGET_MIB)]
    pub artifact_budget_mib: u64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HeadlessAnalysis {
    Diagnose,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FixtureSpec {
    pub schema_version: u32,
    pub route: String,
    pub viewport: FixtureViewport,
    pub stable_state: String,
    #[serde(default)]
    pub allow_visual: bool,
    #[serde(default)]
    pub allow_chromium: bool,
    #[serde(default)]
    pub setup: Option<FixtureCommand>,
    #[serde(default)]
    pub cleanup: Option<FixtureCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FixtureViewport {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_device_scale_factor")]
    pub device_scale_factor: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FixtureCommand {
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_command_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BaselineIndex {
    schema_version: u32,
    states: BTreeMap<String, BaselineLocator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BaselineLocator {
    content_hash: String,
    storage_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct StateIdentityInput<'a> {
    project_key: &'a str,
    route: &'a str,
    viewport: (u32, u32),
    fixture_hash: Option<&'a str>,
    stable_state: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
struct SafeEvidenceSummary {
    id: String,
    kind: String,
    uncertainty: Option<String>,
    revision: Option<String>,
    source: Option<String>,
}

#[derive(Debug)]
struct EvidenceSummary {
    classes: BTreeMap<String, usize>,
    ids: Vec<String>,
    hashes: Vec<String>,
}

#[derive(Debug)]
struct GitProjectState {
    annotation: GitAnnotation,
    working_tree_id: Option<String>,
}

pub async fn run(client: &Client, control: &str, token: &str, args: HeadlessArgs) -> Result<i32> {
    validate_cli_policy(&args)?;
    let sessions: Vec<Session> = authed_get(client, control, token, "/v1/sessions")
        .await?
        .json()
        .await
        .context("invalid LocalView session response")?;
    let session = resolve_session(&sessions, args.session)?;
    validate_local_session(&session)?;

    let project_root = project_root(&session)?;
    let fixture = match &args.fixture {
        Some(path) => Some(load_fixture(&project_root, path).await?),
        None => None,
    };
    if let Some(spec) = &fixture {
        if let Some(command) = &spec.setup {
            run_fixture_command(&project_root, command, "setup").await?;
        }
    }

    let result = run_inner(
        client,
        control,
        token,
        &args,
        &session,
        &project_root,
        fixture.as_ref(),
    )
    .await;

    let cleanup_result =
        if let Some(command) = fixture.as_ref().and_then(|spec| spec.cleanup.as_ref()) {
            run_fixture_command(&project_root, command, "cleanup").await
        } else {
            Ok(())
        };

    match (result, cleanup_result) {
        (Ok(code), Ok(())) => Ok(code),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(cleanup.context("fixture cleanup failed")),
        (Err(error), Err(cleanup)) => Err(anyhow!(
            "{error:#}; fixture cleanup also failed: {cleanup:#}"
        )),
    }
}

async fn run_inner(
    client: &Client,
    control: &str,
    token: &str,
    args: &HeadlessArgs,
    session: &Session,
    project_root: &Path,
    fixture: Option<&FixtureSpec>,
) -> Result<i32> {
    let snapshot: PageSnapshot = authed_get(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/semantic-snapshot/fresh", session.id),
    )
    .await?
    .json()
    .await
    .context("fresh semantic evidence was not a valid bounded PageSnapshot")?;

    let mut incomplete_reasons = Vec::new();
    let mut inconclusive_reasons = Vec::new();
    if let Some(spec) = fixture {
        if snapshot.route != spec.route {
            inconclusive_reasons.push(format!(
                "route drift: expected {}, observed {}",
                bounded_text(&spec.route, 256),
                bounded_text(&snapshot.route, 256)
            ));
        }
        if snapshot.viewport != (spec.viewport.width, spec.viewport.height) {
            inconclusive_reasons.push(format!(
                "viewport drift: expected {}x{}, observed {}x{}",
                spec.viewport.width, spec.viewport.height, snapshot.viewport.0, snapshot.viewport.1
            ));
        }
    }

    let diagnostics: DiagnosticReport = authed_get(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/diagnose", session.id),
    )
    .await?
    .json()
    .await
    .context("invalid LocalView diagnostic report")?;
    let diagnostics = sanitize_diagnostics(diagnostics, project_root);
    let analysis_result = match args.analysis {
        HeadlessAnalysis::Diagnose => "diagnose".to_owned(),
        HeadlessAnalysis::Full => {
            let value = authed_get_value(
                client,
                control,
                token,
                &format!("/v1/sessions/{}/analysis", session.id),
            )
            .await?;
            format!("analysis:{}", compact_status(&value))
        }
    };

    let verification = authed_get_value(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/verify", session.id),
    )
    .await?;
    let verification_verdict = verification
        .get("verdict")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    let proof = authed_post_value(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/proof", session.id),
        None,
    )
    .await?;
    let proof_hashes = proof
        .pointer("/proof/proof_hash")
        .and_then(Value::as_str)
        .map(|value| vec![bounded_text(value, 160)])
        .unwrap_or_default();

    let evidence = authed_get_value(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/evidence/recent", session.id),
    )
    .await?;
    let evidence = summarize_evidence(&evidence);

    let git = read_git_annotation(client, control, token, session.id, &diagnostics).await;
    let revision = git.working_tree_id.clone().or_else(|| {
        verification
            .get("revision")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, 160))
    });

    let fixture_hash = fixture.map(object_hash);
    let state_identity = object_hash(&StateIdentityInput {
        project_key: &session.project.key,
        route: &snapshot.route,
        viewport: snapshot.viewport,
        fixture_hash: fixture_hash.as_deref(),
        stable_state: fixture.map(|spec| spec.stable_state.as_str()),
    });

    let mut visual_result = Value::Null;
    if args.visual {
        match fixture {
            Some(spec) if !visual_permitted(Some(spec)) => {
                inconclusive_reasons.push("visual capture is disabled by fixture policy".into());
            }
            _ => {
                let device_scale_factor = fixture
                    .map(|spec| spec.viewport.device_scale_factor)
                    .unwrap_or(1.0);
                match visual_capture_verify(
                    client,
                    control,
                    token,
                    session.id,
                    snapshot.viewport,
                    device_scale_factor,
                    revision.as_deref(),
                    args.visual_max_changed_ratio,
                )
                .await
                {
                    Ok(value) => visual_result = value,
                    Err(VisualRequestError::ResourceDenied) => inconclusive_reasons
                        .push("visual capture denied by Runtime Resource Governor".into()),
                    Err(VisualRequestError::Unavailable(reason)) => {
                        inconclusive_reasons.push(reason)
                    }
                    Err(VisualRequestError::Fatal(error)) => return Err(error),
                }
            }
        }
    }

    let mut chromium_result = Value::Null;
    if args.chromium {
        match fixture {
            Some(spec) if !chromium_permitted(Some(spec)) => {
                inconclusive_reasons.push("Chromium is disabled by fixture policy".into());
            }
            _ => {
                match request_bounded_chromium_cycle(
                    client,
                    control,
                    token,
                    session.id,
                    revision.as_deref(),
                )
                .await
                {
                    Ok(value) => chromium_result = value,
                    Err(VisualRequestError::ResourceDenied) => inconclusive_reasons
                        .push("Chromium denied by Runtime Resource Governor".into()),
                    Err(VisualRequestError::Unavailable(reason)) => {
                        inconclusive_reasons.push(reason)
                    }
                    Err(VisualRequestError::Fatal(error)) => return Err(error),
                }
            }
        }
    }

    let state_root = project_root.join(".localview").join("wave8-ci");
    let artifact_root = state_root.join("artifacts");
    tokio::fs::create_dir_all(&artifact_root).await?;
    let budget_bytes = args
        .artifact_budget_mib
        .checked_mul(1024 * 1024)
        .ok_or_else(|| anyhow!("artifact budget overflows byte accounting"))?;
    let mut artifact_store = ArtifactStore::open(&artifact_root, budget_bytes).await?;

    let baseline_envelope = BaselineEnvelope {
        schema_version: 1,
        state_identity: state_identity.clone(),
        route: snapshot.route.clone(),
        viewport: snapshot.viewport,
        evidence_hashes: evidence.hashes.clone(),
        design_baseline_hash: None,
        created_revision: revision.clone(),
        provenance: BTreeMap::from([
            ("source".into(), "wave8-headless-ci".into()),
            ("session".into(), session.id.to_string()),
        ]),
    }
    .normalized();
    let (baseline, baseline_artifact) = compare_and_retain_baseline(
        &state_root,
        &artifact_root,
        &mut artifact_store,
        &baseline_envelope,
        args.update_baseline,
    )
    .await?;

    if baseline.status == BaselineComparisonStatus::Incompatible {
        inconclusive_reasons
            .push("baseline is unavailable or failed canonical validation".into());
    }

    if verification_verdict == "inconclusive"
        || verification_verdict == "stale"
        || verification_verdict == "unknown"
    {
        incomplete_reasons.push(format!("verification verdict is {verification_verdict}"));
    }

    let (status, exit_code) = evaluate_exit_policy(
        args,
        &diagnostics,
        &verification_verdict,
        baseline.status,
        !inconclusive_reasons.is_empty(),
    );

    let mut metadata = BTreeMap::new();
    metadata.insert("analysis".into(), analysis_result);
    metadata.insert("visual_requested".into(), args.visual.to_string());
    metadata.insert("chromium_requested".into(), args.chromium.to_string());
    metadata.insert("visual_result".into(), compact_status(&visual_result));
    metadata.insert("chromium_result".into(), compact_status(&chromium_result));
    metadata.insert(
        "artifact_retained_bytes".into(),
        artifact_store.used_bytes().to_string(),
    );

    let artifacts = baseline_artifact.into_iter().collect::<Vec<_>>();
    let mut report = LocalViewReport {
        schema_version: 1,
        title: "LocalView Headless CI Report".into(),
        generated_at: generated_at(),
        status,
        project: bounded_text(&session.project.display_name, 160),
        project_key: bounded_text(&session.project.key, 160),
        session_id: session.id.to_string(),
        revision: revision.clone(),
        state_identity: state_identity.clone(),
        route: bounded_text(&snapshot.route, 512),
        viewport: Some(snapshot.viewport),
        evidence_classes: evidence.classes,
        evidence_ids: evidence.ids,
        diagnostics,
        verification,
        baseline,
        artifacts,
        incomplete_reasons,
        inconclusive_reasons,
        git: git.annotation,
        metadata,
    };
    report.normalize();

    let report_hash = object_hash(&report);
    let attestation = digest_attestation(DigestAttestationPayload {
        schema_version: 1,
        report_hash: report_hash.clone(),
        revision,
        state_identity,
        evidence_hashes: evidence.hashes,
        proof_hashes,
        gate_status: match status {
            ReportStatus::Passed => "passed",
            ReportStatus::Failed => "failed",
            ReportStatus::Inconclusive => "inconclusive",
            ReportStatus::InfrastructureError => "infrastructure_error",
        }
        .into(),
        environment_fingerprint: BTreeMap::from([
            ("os".into(), std::env::consts::OS.into()),
            ("arch".into(), std::env::consts::ARCH.into()),
            ("ci".into(), std::env::var_os("CI").is_some().to_string()),
        ]),
    });

    let output_dir = resolve_output_dir(project_root, args.output_dir.as_deref()).await?;
    write_bundle(&output_dir, &report, &attestation).await?;
    emit_ci_annotations(&report);
    println!(
        "LocalView headless: status={:?} exit={} report_hash={} output={}",
        report.status,
        exit_code,
        report_hash,
        display_project_relative(project_root, &output_dir)
    );
    Ok(exit_code)
}

fn validate_cli_policy(args: &HeadlessArgs) -> Result<()> {
    if !(1..=3).contains(&args.deterministic_severity) {
        bail!("--deterministic-severity must be in 1..=3");
    }
    if !args.visual_max_changed_ratio.is_finite()
        || !(0.0..=1.0).contains(&args.visual_max_changed_ratio)
