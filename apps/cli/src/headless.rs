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
use localview_diagnostics::{DiagnosticClass, DiagnosticIssue, DiagnosticReport};
use localview_live_analysis::{FindingClass, LiveDiagnosis};
use localview_protocol::{PageSnapshot, Session, SessionId};
use localview_reports::{
    ArtifactReference, BaselineComparison, BaselineComparisonStatus, GitAnnotation,
    LocalViewReport, ReportStatus, render_html, render_json, render_markdown,
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
    pub allow_fixture_command: bool,
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
    #[arg(long)]
    pub visual: bool,
    #[arg(long)]
    pub chromium: bool,
    #[arg(long)]
    pub update_baseline: bool,
    #[arg(long)]
    pub design_baseline_hash: Option<String>,
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
    kind: String,
    uncertainty: Option<String>,
    revision: Option<String>,
    source: Option<String>,
    payload_hash: String,
}

#[derive(Debug)]
struct EvidenceSummary {
    classes: BTreeMap<String, usize>,
    ids: Vec<String>,
    hashes: Vec<String>,
    baseline_hashes: Vec<String>,
    source_files: Vec<String>,
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
        if (spec.setup.is_some() || spec.cleanup.is_some()) && !args.allow_fixture_command {
            bail!("fixture setup/cleanup commands require explicit --allow-fixture-command policy");
        }
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
    let fixture_state_matches = fixture_matches_snapshot(fixture, &snapshot);
    if let Some(spec) = fixture {
        if snapshot.route != spec.route {
            inconclusive_reasons.push(format!(
                "route drift: expected {}, observed {}",
                safe_route(&spec.route),
                safe_route(&snapshot.route)
            ));
        }
        if snapshot.viewport != (spec.viewport.width, spec.viewport.height) {
            inconclusive_reasons.push(format!(
                "viewport drift: expected {}x{}, observed {}x{}",
                spec.viewport.width, spec.viewport.height, snapshot.viewport.0, snapshot.viewport.1
            ));
        }
    }

    let diagnosis: LiveDiagnosis = authed_get(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/diagnose", session.id),
    )
    .await?
    .json()
    .await
    .context("invalid LocalView live diagnosis")?;
    for unknown in &diagnosis.unknowns {
        incomplete_reasons.push(format!(
            "{}: {}",
            bounded_text(&unknown.statement, 256),
            bounded_text(&unknown.reason, 256)
        ));
    }
    let diagnostics = sanitize_diagnostics(diagnostic_report_from_live(&diagnosis), project_root);
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

    let mut git = read_git_annotation(client, control, token, session.id, &diagnostics).await;
    let mut revision = git.working_tree_id.clone();

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
                    VisualCaptureVerifyInput {
                        session: session.id,
                        viewport: snapshot.viewport,
                        device_scale_factor,
                        revision: revision.as_deref(),
                        max_changed_ratio: args.visual_max_changed_ratio,
                    },
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

    let visual_verdict = visual_result
        .pointer("/result/verdict")
        .and_then(Value::as_str);
    let visual_failed = visual_verdict == Some("fail");
    if visual_verdict == Some("inconclusive") {
        let reason = visual_result
            .pointer("/result/reason")
            .and_then(Value::as_str)
            .unwrap_or("visual verification was inconclusive");
        inconclusive_reasons.push(format!("visual verification: {}", bounded_text(reason, 256)));
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
                    Ok(value) => {
                        if !chromium_cycle_executed(&value) {
                            inconclusive_reasons.push(
                                "Chromium was requested but the bounded perception cycle produced no chromium_compatibility receipt"
                                    .into(),
                            );
                        }
                        chromium_result = value;
                    },
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

    let final_snapshot: PageSnapshot = authed_get(
        client,
        control,
        token,
        &format!("/v1/sessions/{}/semantic-snapshot/fresh", session.id),
    )
    .await?
    .json()
    .await
    .context("final semantic evidence was not a valid bounded PageSnapshot")?;
    let initial_semantic_hash = semantic_state_hash(&snapshot);
    let final_semantic_hash = semantic_state_hash(&final_snapshot);
    let state_stable = headless_state_stable(&snapshot, &final_snapshot);
    if !state_stable {
        inconclusive_reasons.push(format!(
            "headless state drifted during execution: route {} -> {}, viewport {}x{} -> {}x{}, semantic_state_changed={}",
            safe_route(&snapshot.route),
            safe_route(&final_snapshot.route),
            snapshot.viewport.0,
            snapshot.viewport.1,
            final_snapshot.viewport.0,
            final_snapshot.viewport.1,
            initial_semantic_hash != final_semantic_hash
        ));
    }

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
    if revision.is_none() {
        revision = verification
            .get("revision")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, 160));
    }

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
    let baseline_evidence_ids =
        baseline_evidence_ids(&verification, &visual_result, &chromium_result);
    let evidence = summarize_evidence(&evidence, &baseline_evidence_ids);
    git.annotation
        .relevant_source_files
        .extend(evidence.source_files.iter().cloned());
    git.annotation.relevant_source_files.sort();
    git.annotation.relevant_source_files.dedup();
    git.annotation
        .relevant_source_files
        .truncate(MAX_RELEVANT_FILES);

    let state_root = project_root.join(".localview").join("wave8-ci");
    let artifact_root = state_root.join("artifacts");
    tokio::fs::create_dir_all(&artifact_root).await?;
    let budget_bytes = args
        .artifact_budget_mib
        .checked_mul(1024 * 1024)
        .ok_or_else(|| anyhow!("artifact budget overflows byte accounting"))?;
    let mut artifact_store = ArtifactStore::open(&artifact_root, budget_bytes).await?;

    let mut baseline_evidence_hashes = evidence.baseline_hashes.clone();
    baseline_evidence_hashes.push(final_semantic_hash.clone());
    baseline_evidence_hashes.sort();
    baseline_evidence_hashes.dedup();
    let baseline_envelope = BaselineEnvelope {
        schema_version: 1,
        state_identity: state_identity.clone(),
        route: safe_route(&snapshot.route),
        viewport: snapshot.viewport,
        evidence_hashes: baseline_evidence_hashes,
        design_baseline_hash: args.design_baseline_hash.clone(),
        created_revision: revision.clone(),
        provenance: BTreeMap::from([
            ("source".into(), "wave8-headless-ci".into()),
            ("session".into(), session.id.to_string()),
            ("observed_state_hash".into(), final_semantic_hash.clone()),
        ]),
    }
    .normalized();
    let (baseline, baseline_artifact) = compare_and_retain_baseline(
        &state_root,
        &artifact_root,
        &mut artifact_store,
        &baseline_envelope,
        state_stable && fixture_state_matches,
        args.update_baseline,
    )
    .await?;

    if baseline.status == BaselineComparisonStatus::Incompatible {
        inconclusive_reasons.push("baseline is unavailable or failed canonical validation".into());
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
        visual_failed,
        !inconclusive_reasons.is_empty(),
    );

    let mut metadata = BTreeMap::new();
    metadata.insert("analysis".into(), analysis_result);
    metadata.insert("visual_requested".into(), args.visual.to_string());
    metadata.insert("chromium_requested".into(), args.chromium.to_string());
    metadata.insert("visual_result".into(), compact_status(&visual_result));
    metadata.insert(
        "chromium_executed".into(),
        chromium_cycle_executed(&chromium_result).to_string(),
    );
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
    {
        bail!("--visual-max-changed-ratio must be finite and within 0..=1");
    }
    if args.artifact_budget_mib == 0 || args.artifact_budget_mib > 4096 {
        bail!("--artifact-budget-mib must be within 1..=4096");
    }
    if args
        .design_baseline_hash
        .as_deref()
        .is_some_and(|value| !valid_sha256_hash(value))
    {
        bail!("--design-baseline-hash must be a canonical sha256:<64 hex> hash");
    }
    Ok(())
}

fn visual_permitted(fixture: Option<&FixtureSpec>) -> bool {
    fixture.is_none_or(|spec| spec.allow_visual)
}

fn chromium_permitted(fixture: Option<&FixtureSpec>) -> bool {
    fixture.is_none_or(|spec| spec.allow_chromium)
}

fn evaluate_exit_policy(
    args: &HeadlessArgs,
    diagnostics: &DiagnosticReport,
    verification_verdict: &str,
    baseline_status: BaselineComparisonStatus,
    visual_failed: bool,
    has_inconclusive_reason: bool,
) -> (ReportStatus, i32) {
    let hard_deterministic = diagnostics.issues.iter().any(|issue| {
        issue.class == DiagnosticClass::Deterministic
            && issue.severity >= args.deterministic_severity
    });
    let hard_heuristic = args.fail_on_heuristic
        && diagnostics
            .issues
            .iter()
            .any(|issue| issue.class == DiagnosticClass::Heuristic);
    let verification_failed = verification_verdict == "fail";
    let baseline_failed = args.require_baseline_match
        && baseline_status == BaselineComparisonStatus::Changed
        && !args.update_baseline;
    let verification_incomplete = args.require_verification_pass && verification_verdict != "pass";

    if hard_deterministic
        || hard_heuristic
        || verification_failed
        || baseline_failed
        || visual_failed
    {
        (ReportStatus::Failed, EXIT_HARD_FAILURE)
    } else if verification_incomplete || has_inconclusive_reason {
        (ReportStatus::Inconclusive, EXIT_INCONCLUSIVE)
    } else {
        (ReportStatus::Passed, EXIT_PASS)
    }
}

fn resolve_session(sessions: &[Session], requested: Option<SessionId>) -> Result<Session> {
    if let Some(id) = requested {
        return sessions
            .iter()
            .find(|session| session.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("requested LocalView session {id} is not active"));
    }
    match sessions {
        [] => bail!("no LocalView sessions are active"),
        [session] => Ok(session.clone()),
        _ => bail!("multiple LocalView sessions are active; pass --session explicitly"),
    }
}

fn validate_local_session(session: &Session) -> Result<()> {
    let host = session.endpoint.host.trim_matches(['[', ']']);
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        bail!("headless mode refuses non-loopback session target");
    }
    if !matches!(session.endpoint.scheme.as_str(), "http" | "https") {
        bail!("headless mode requires an HTTP(S) localhost target");
    }
    Ok(())
}

fn project_root(session: &Session) -> Result<PathBuf> {
    let raw = session
        .project
        .git_root
        .as_deref()
        .or(session.project.cwd.as_deref())
        .ok_or_else(|| {
            anyhow!("session has no project root; headless CI cannot bind fixture state")
        })?;
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        bail!("session project root is not absolute");
    }
    Ok(path)
}

async fn load_fixture(project_root: &Path, requested: &Path) -> Result<FixtureSpec> {
    let path = contained_existing_path(project_root, requested)?;
    let metadata = tokio::fs::metadata(&path).await?;
    if metadata.len() > MAX_FIXTURE_BYTES {
        bail!("fixture exceeds {MAX_FIXTURE_BYTES} bytes");
    }
    let bytes = tokio::fs::read(&path).await?;
    let spec: FixtureSpec =
        serde_json::from_slice(&bytes).context("invalid Wave 8 fixture JSON")?;
    validate_fixture(&spec)?;
    Ok(spec)
}

fn validate_fixture(spec: &FixtureSpec) -> Result<()> {
    if spec.schema_version != 1 {
        bail!("unsupported fixture schema_version {}", spec.schema_version);
    }
    if spec.route.is_empty() || spec.route.len() > 1000 || !spec.route.starts_with('/') {
        bail!("fixture route must be a bounded local route beginning with '/'");
    }
    if spec.viewport.width == 0
        || spec.viewport.height == 0
        || spec.viewport.width > 100_000
        || spec.viewport.height > 100_000
        || !spec.viewport.device_scale_factor.is_finite()
        || spec.viewport.device_scale_factor <= 0.0
        || spec.viewport.device_scale_factor > 8.0
    {
        bail!("fixture viewport is invalid");
    }
    if spec.stable_state.is_empty() || spec.stable_state.len() > 256 {
        bail!("fixture stable_state must be 1..=256 bytes");
    }
    for command in [spec.setup.as_ref(), spec.cleanup.as_ref()]
        .into_iter()
        .flatten()
    {
        validate_fixture_command(command)?;
    }
    Ok(())
}

fn validate_fixture_command(command: &FixtureCommand) -> Result<()> {
    if command.executable.is_empty() || command.executable.len() > 128 {
        bail!("fixture executable is invalid");
    }
    if command.executable.contains(['/', '\\']) {
        bail!("fixture executable must be resolved from PATH, not an arbitrary path");
    }
    let executable = command.executable.to_ascii_lowercase();
    if matches!(
        executable.as_str(),
        "sh" | "bash" | "zsh" | "fish" | "cmd" | "cmd.exe" | "powershell" | "pwsh"
    ) {
        bail!("shell interpreters are not allowed in fixture commands");
    }
    if command.args.len() > 64 || command.args.iter().any(|arg| arg.len() > 4096) {
        bail!("fixture command arguments exceed bounded policy");
    }
    if command.timeout_ms == 0 || command.timeout_ms > MAX_COMMAND_TIMEOUT_MS {
        bail!("fixture command timeout must be within 1..={MAX_COMMAND_TIMEOUT_MS} ms");
    }
    if let Some(cwd) = &command.cwd {
        validate_relative_path(Path::new(cwd))?;
    }
    Ok(())
}

async fn run_fixture_command(
    project_root: &Path,
    command: &FixtureCommand,
    phase: &str,
) -> Result<()> {
    validate_fixture_command(command)?;
    let cwd = match &command.cwd {
        Some(relative) => contained_existing_path(project_root, Path::new(relative))?,
        None => project_root.to_path_buf(),
    };
    let mut child = Command::new(&command.executable)
        .args(&command.args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("cannot start fixture {phase} command"))?;
    let wait = timeout(Duration::from_millis(command.timeout_ms), child.wait()).await;
    let status = match wait {
        Ok(result) => result.with_context(|| format!("fixture {phase} command wait failed"))?,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            bail!(
                "fixture {phase} command timed out after {} ms",
                command.timeout_ms
            );
        }
    };
    if !status.success() {
        bail!("fixture {phase} command exited unsuccessfully");
    }
    Ok(())
}

async fn read_git_annotation(
    client: &Client,
    control: &str,
    token: &str,
    session: SessionId,
    diagnostics: &DiagnosticReport,
) -> GitProjectState {
    let response = authed_get_raw(
        client,
        control,
        token,
        &format!("/v1/sessions/{session}/project-state"),
    )
    .await;
    let Ok(response) = response else {
        return unavailable_git_project_state();
    };
    if !response.status().is_success() {
        return unavailable_git_project_state();
    }
    let Ok(value) = response.json::<Value>().await else {
        return unavailable_git_project_state();
    };
    git_project_state_from_value(&value, diagnostics)
}

fn unavailable_git_project_state() -> GitProjectState {
    GitProjectState {
        annotation: GitAnnotation {
            unavailable_reason: Some("git unavailable".into()),
            ..GitAnnotation::default()
        },
        working_tree_id: None,
    }
}

fn git_project_state_from_value(value: &Value, diagnostics: &DiagnosticReport) -> GitProjectState {
    let changed_files = value
        .get("dirty_files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|path| safe_relative_report_path(path))
        .take(MAX_CHANGED_FILES)
        .map(|path| bounded_text(path, 512))
        .collect::<Vec<_>>();
    let relevant_source_files = relevant_source_files(diagnostics);
    GitProjectState {
        annotation: GitAnnotation {
            available: true,
            revision: value
                .get("commit")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 160)),
            branch: value
                .get("branch")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 160)),
            dirty: Some(!changed_files.is_empty()),
            changed_files,
            relevant_source_files,
            unavailable_reason: None,
        },
        working_tree_id: value
            .get("working_tree_id")
            .and_then(Value::as_str)
            .map(|value| bounded_text(value, 200)),
    }
}

fn relevant_source_files(diagnostics: &DiagnosticReport) -> Vec<String> {
    let mut files = diagnostics
        .issues
        .iter()
        .flat_map(|issue| issue.refs.iter().chain(issue.evidence.iter()))
        .filter(|value| safe_relative_report_path(value))
        .filter(|value| {
            matches!(
                Path::new(value).extension().and_then(|ext| ext.to_str()),
                Some(
                    "rs" | "ts" | "tsx" | "js" | "jsx" | "vue" | "svelte" | "css" | "scss" | "html"
                )
            )
        })
        .take(MAX_RELEVANT_FILES)
        .map(|value| bounded_text(value, 512))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn baseline_evidence_ids(
    verification: &Value,
    visual_result: &Value,
    chromium_result: &Value,
) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    if let Some(fresh) = verification.get("fresh_evidence_ids").and_then(Value::as_array) {
        ids.extend(fresh.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    if let Some(id) = visual_result.get("evidence_id").and_then(Value::as_str) {
        ids.insert(id.to_owned());
    }
    if let Some(steps) = chromium_result.get("steps").and_then(Value::as_array) {
        for step in steps {
            if step.pointer("/execution/kind").and_then(Value::as_str)
                == Some("chromium_compatibility")
            {
                if let Some(id) = step
                    .pointer("/execution/evidence_id")
                    .and_then(Value::as_str)
                {
                    ids.insert(id.to_owned());
                }
            }
        }
    }
    ids
}

fn summarize_evidence(
    value: &Value,
    baseline_evidence_ids: &std::collections::BTreeSet<String>,
) -> EvidenceSummary {
    let mut classes = BTreeMap::new();
    let mut ids = Vec::new();
    let mut hashes = Vec::new();
    let mut baseline_hashes = Vec::new();
    let mut source_files = std::collections::BTreeSet::new();
    let Some(items) = value.as_array() else {
        return EvidenceSummary {
            classes,
            ids,
            hashes,
            baseline_hashes,
            source_files: Vec::new(),
        };
    };
    for item in items.iter().rev().take(MAX_EVIDENCE).rev() {
        if item.get("secret_taint").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(kind) = item.get("kind").and_then(Value::as_str) else {
            continue;
        };
        *classes.entry(kind.to_owned()).or_insert(0) += 1;
        ids.push(bounded_text(id, 160));
        let safe = SafeEvidenceSummary {
            kind: bounded_text(kind, 64),
            uncertainty: item
                .get("uncertainty")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 64)),
            revision: item
                .pointer("/provenance/revision")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 160)),
            source: item
                .pointer("/provenance/source")
                .and_then(Value::as_str)
                .map(|value| bounded_text(value, 96)),
            payload_hash: object_hash(item.get("payload").unwrap_or(&Value::Null)),
        };
        let safe_hash = object_hash(&safe);
        hashes.push(safe_hash.clone());
        if baseline_evidence_ids.contains(id) {
            baseline_hashes.push(safe_hash);
        }
        collect_evidence_source_files(
            item.get("payload").unwrap_or(&Value::Null),
            &mut source_files,
            0,
        );
    }
    ids.sort();
    ids.dedup();
    hashes.sort();
    hashes.dedup();
    baseline_hashes.sort();
    baseline_hashes.dedup();
    EvidenceSummary {
        classes,
        ids,
        hashes,
        baseline_hashes,
        source_files: source_files.into_iter().collect(),
    }
}

fn collect_evidence_source_files(
    value: &Value,
    output: &mut std::collections::BTreeSet<String>,
    depth: usize,
) {
    if depth > 6 || output.len() >= MAX_RELEVANT_FILES {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if matches!(key.as_str(), "file" | "source_file" | "sourceFile") {
                    if let Some(path) = value.as_str() {
                        if safe_relative_report_path(path) {
                            output.insert(bounded_text(path, 512));
                        }
                    }
                }
                collect_evidence_source_files(value, output, depth + 1);
            }
        }
        Value::Array(values) => {
            for value in values.iter().take(128) {
                collect_evidence_source_files(value, output, depth + 1);
            }
        }
        _ => {}
    }
}

fn diagnostic_report_from_live(diagnosis: &LiveDiagnosis) -> DiagnosticReport {
    let issues = diagnosis
        .findings
        .iter()
        .map(|finding| DiagnosticIssue {
            category: finding.category.clone(),
            code: finding.code.clone(),
            message: finding.message.clone(),
            severity: finding.severity,
            confidence: finding.confidence,
            class: match finding.class {
                FindingClass::Deterministic => DiagnosticClass::Deterministic,
                FindingClass::Heuristic => DiagnosticClass::Heuristic,
            },
            refs: Vec::new(),
            evidence: None,
        })
        .collect::<Vec<_>>();
    let deterministic = issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Deterministic)
        .count();
    let heuristic = issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Heuristic)
        .count();
    DiagnosticReport {
        issues,
        deterministic,
        heuristic,
        subjective: 0,
    }
}

fn sanitize_diagnostics(mut report: DiagnosticReport, project_root: &Path) -> DiagnosticReport {
    let root = project_root.to_string_lossy();
    for issue in &mut report.issues {
        issue.category = sanitize_text(&issue.category, &root, 96);
        issue.code = sanitize_text(&issue.code, &root, 160);
        issue.message = sanitize_text(&issue.message, &root, MAX_DIAGNOSTIC_TEXT);
        issue.refs = issue
            .refs
            .iter()
            .take(64)
            .map(|value| sanitize_text(value, &root, 256))
            .collect();
        issue.evidence = issue
            .evidence
            .as_deref()
            .map(|value| sanitize_text(value, &root, MAX_DIAGNOSTIC_TEXT));
    }
    report.deterministic = report
        .issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Deterministic)
        .count();
    report.heuristic = report
        .issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Heuristic)
        .count();
    report.subjective = report
        .issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Subjective)
        .count();
    report
}

fn sanitize_text(value: &str, project_root: &str, max: usize) -> String {
    let replaced = if project_root.is_empty() {
        value.to_owned()
    } else {
        value.replace(project_root, "<project>")
    };
    bounded_text(&replaced.replace(['\r', '\n'], " "), max)
}

fn chromium_cycle_executed(value: &Value) -> bool {
    value
        .get("steps")
        .and_then(Value::as_array)
        .is_some_and(|steps| {
            steps.iter().any(|step| {
                step.pointer("/execution/kind").and_then(Value::as_str)
                    == Some("chromium_compatibility")
            })
        })
}

struct VisualCaptureVerifyInput<'a> {
    session: SessionId,
    viewport: (u32, u32),
    device_scale_factor: f64,
    revision: Option<&'a str>,
    max_changed_ratio: f64,
}

async fn visual_capture_verify(
    client: &Client,
    control: &str,
    token: &str,
    input: VisualCaptureVerifyInput<'_>,
) -> std::result::Result<Value, VisualRequestError> {
    let body = json!({
        "viewport": {
            "css_width": input.viewport.0,
            "css_height": input.viewport.1,
            "device_scale_factor": input.device_scale_factor,
        },
        "revision": input.revision,
        "expectation": {
            "kind": "unchanged",
            "max_changed_ratio": input.max_changed_ratio,
        }
    });
    let path = format!("/v1/sessions/{}/verify/visual/capture", input.session);
    let first = authed_post_raw(client, control, token, &path, Some(&body))
        .await
        .map_err(VisualRequestError::Fatal)?;
    let first = classify_visual_response(first).await?;
    if !visual_result_needs_baseline_retry(&first) {
        return Ok(first);
    }
    let second = authed_post_raw(client, control, token, &path, Some(&body))
        .await
        .map_err(VisualRequestError::Fatal)?;
    classify_visual_response(second).await
}

fn visual_result_needs_baseline_retry(value: &Value) -> bool {
    value.pointer("/result/verdict").and_then(Value::as_str) == Some("inconclusive")
        && value.pointer("/result/reason").and_then(Value::as_str)
            == Some("visual assertion has no comparable baseline")
}

async fn request_bounded_chromium_cycle(
    client: &Client,
    control: &str,
    token: &str,
    session: SessionId,
    revision: Option<&str>,
) -> std::result::Result<Value, VisualRequestError> {
    let body = json!({
        "budget": {
            "latency_ms": 5_000,
            "text_tokens": 800,
            "image_regions": 0,
            "chromium_spawns": 1
        },
        "deep_mode": false,
        "compatibility_requested": true,
        "target": Value::Null,
        "revision": revision,
    });
    let response = authed_post_raw(
        client,
        control,
        token,
        &format!("/v1/sessions/{session}/perception/cycle"),
        Some(&body),
    )
    .await
    .map_err(VisualRequestError::Fatal)?;
    classify_visual_response(response).await
}

#[derive(Debug)]
enum VisualRequestError {
    ResourceDenied,
    Unavailable(String),
    Fatal(anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundedRequestDisposition {
    Success,
    ResourceDenied,
    Unavailable(String),
    Fatal(String),
}

fn classify_control_status(status: StatusCode, value: &Value) -> BoundedRequestDisposition {
    if status == StatusCode::TOO_MANY_REQUESTS
        && value.get("error").and_then(Value::as_str) == Some("resource_governor_denied")
    {
        return BoundedRequestDisposition::ResourceDenied;
    }
    if matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::GATEWAY_TIMEOUT | StatusCode::SERVICE_UNAVAILABLE
    ) {
        let reason = value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("visual_or_chromium_unavailable");
        return BoundedRequestDisposition::Unavailable(bounded_text(reason, 160));
    }
    if !status.is_success() {
        return BoundedRequestDisposition::Fatal(format!(
            "headless control request failed with HTTP {status}: {}",
            compact_status(value)
        ));
    }
    BoundedRequestDisposition::Success
}

async fn classify_visual_response(
    response: Response,
) -> std::result::Result<Value, VisualRequestError> {
    let status = response.status();
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    match classify_control_status(status, &value) {
        BoundedRequestDisposition::Success => Ok(value),
        BoundedRequestDisposition::ResourceDenied => Err(VisualRequestError::ResourceDenied),
        BoundedRequestDisposition::Unavailable(reason) => {
            Err(VisualRequestError::Unavailable(reason))
        }
        BoundedRequestDisposition::Fatal(message) => {
            Err(VisualRequestError::Fatal(anyhow!(message)))
        }
    }
}

async fn compare_and_retain_baseline(
    state_root: &Path,
    artifact_root: &Path,
    store: &mut ArtifactStore,
    candidate: &BaselineEnvelope,
    retain_allowed: bool,
    update: bool,
) -> Result<(BaselineComparison, Option<ArtifactReference>)> {
    tokio::fs::create_dir_all(state_root).await?;
    let index_path = state_root.join("baseline-index.json");
    let mut index = load_baseline_index(&index_path).await?;
    let candidate_hash = candidate.canonical_hash();
    let existing = index.states.get(&candidate.state_identity).cloned();

    if !retain_allowed {
        return Ok((
            BaselineComparison {
                status: BaselineComparisonStatus::Incompatible,
                baseline_hash: existing
                    .as_ref()
                    .map(|locator| locator.content_hash.clone()),
                candidate_hash: Some(candidate_hash),
                reasons: vec!["fixture or headless state drifted; baseline authority was withheld".into()],
            },
            None,
        ));
    }

    let mut comparison = BaselineComparison {
        status: BaselineComparisonStatus::Created,
        baseline_hash: None,
        candidate_hash: Some(candidate_hash.clone()),
        reasons: Vec::new(),
    };
    let mut should_store = existing.is_none();

    if let Some(locator) = existing {
        comparison.baseline_hash = Some(locator.content_hash.clone());
        match load_retained_baseline(artifact_root, &locator).await {
            Ok(baseline) => {
                if baseline.state_identity != candidate.state_identity
                    || baseline.route != candidate.route
                    || baseline.viewport != candidate.viewport
                {
                    comparison.status = BaselineComparisonStatus::Incompatible;
                    comparison
                        .reasons
                        .push("retained baseline identity/route/viewport is incompatible".into());
                } else if baseline.evidence_hashes == candidate.evidence_hashes
                    && baseline.design_baseline_hash == candidate.design_baseline_hash
                {
                    comparison.status = BaselineComparisonStatus::Match;
                } else {
                    comparison.status = BaselineComparisonStatus::Changed;
                    comparison
                        .reasons
                        .push("canonical evidence dependency set changed".into());
                    should_store = update;
                }
            }
            Err(reason) => {
                comparison.status = BaselineComparisonStatus::Incompatible;
                comparison.reasons.push(reason);
                should_store = update;
            }
        }
    }

    if !should_store {
        return Ok((comparison, None));
    }

    let bytes = serde_json::to_vec(candidate)?;
    if bytes.len() as u64 > MAX_BASELINE_BYTES {
        bail!("baseline envelope exceeds retained size policy");
    }
    let retained = store
        .put_canonical("wave8/baseline-json", &candidate_hash, &bytes)
        .await?;
    let meta = retained.physical;
    index.schema_version = 1;
    index.states.insert(
        candidate.state_identity.clone(),
        BaselineLocator {
            content_hash: retained.canonical_hash,
            storage_id: meta.id.clone(),
        },
    );
    write_baseline_index(&index_path, &index).await?;
    if update && comparison.status == BaselineComparisonStatus::Changed {
        comparison
            .reasons
            .push("baseline updated by explicit --update-baseline policy".into());
    }
    Ok((
        comparison,
        Some(ArtifactReference {
            kind: "baseline".into(),
            storage_id: meta.id,
            content_hash: candidate_hash,
            bytes: meta.bytes,
        }),
    ))
}

async fn load_baseline_index(path: &Path) -> Result<BaselineIndex> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            if bytes.len() as u64 > MAX_BASELINE_BYTES {
                bail!("baseline index exceeds bounded size policy");
            }
            let index: BaselineIndex = serde_json::from_slice(&bytes)?;
            if index.schema_version != 0 && index.schema_version != 1 {
                bail!("unsupported baseline index schema version");
            }
            Ok(index)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BaselineIndex {
            schema_version: 1,
            states: BTreeMap::new(),
        }),
        Err(error) => Err(error.into()),
    }
}

async fn write_baseline_index(path: &Path, index: &BaselineIndex) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(index)?;
    let temp = path.with_extension("json.tmp");
    tokio::fs::write(&temp, bytes).await?;
    tokio::fs::rename(&temp, path).await?;
    Ok(())
}

async fn load_retained_baseline(
    artifact_root: &Path,
    locator: &BaselineLocator,
) -> std::result::Result<BaselineEnvelope, String> {
    if !valid_physical_artifact_id(&locator.storage_id) {
        return Err("baseline locator has invalid physical artifact id".into());
    }
    let path = artifact_root.join(&locator.storage_id);
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|_| "retained baseline artifact is missing under bounded retention".to_owned())?;
    if metadata.len() > MAX_BASELINE_BYTES {
        return Err("retained baseline artifact exceeds bounded size policy".into());
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| "retained baseline artifact is unreadable".to_owned())?;
    let baseline: BaselineEnvelope = serde_json::from_slice(&bytes)
        .map_err(|_| "retained baseline artifact is invalid JSON".to_owned())?;
    if baseline.canonical_hash() != locator.content_hash {
        return Err("retained baseline canonical hash does not match locator".into());
    }
    Ok(baseline)
}

fn valid_physical_artifact_id(value: &str) -> bool {
    value.len() == 19
        && value.starts_with("lv-")
        && value[3..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn resolve_output_dir(project_root: &Path, requested: Option<&Path>) -> Result<PathBuf> {
    let path = requested.map(Path::to_path_buf).unwrap_or_else(|| {
        project_root
            .join(".localview")
            .join("wave8-ci")
            .join("reports")
    });
    let path = if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    };
    validate_lexically_contained(project_root, &path)?;
    tokio::fs::create_dir_all(&path).await?;
    let canonical = tokio::fs::canonicalize(&path).await?;
    let canonical_root = tokio::fs::canonicalize(project_root).await?;
    if !canonical.starts_with(&canonical_root) {
        bail!("output directory must remain inside project root");
    }
    Ok(canonical)
}

async fn write_bundle<T: Serialize>(
    output_dir: &Path,
    report: &LocalViewReport,
    attestation: &T,
) -> Result<()> {
    tokio::fs::write(output_dir.join("report.json"), render_json(report)?).await?;
    tokio::fs::write(output_dir.join("report.md"), render_markdown(report)).await?;
    tokio::fs::write(output_dir.join("report.html"), render_html(report)).await?;
    tokio::fs::write(
        output_dir.join("attestation.json"),
        serde_json::to_string_pretty(attestation)?,
    )
    .await?;
    Ok(())
}

fn emit_ci_annotations(report: &LocalViewReport) {
    println!(
        "localview-ci status={:?} deterministic={} heuristic={} subjective={}",
        report.status,
        report.diagnostics.deterministic,
        report.diagnostics.heuristic,
        report.diagnostics.subjective
    );
    if std::env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true") {
        return;
    }
    for issue in report
        .diagnostics
        .issues
        .iter()
        .filter(|issue| issue.class == DiagnosticClass::Deterministic && issue.severity >= 3)
        .take(20)
    {
        let message = github_command_escape(&format!("{}: {}", issue.code, issue.message));
        println!("::error title=LocalView deterministic gate::{message}");
    }
    for reason in report.inconclusive_reasons.iter().take(20) {
        println!(
            "::warning title=LocalView inconclusive::{}",
            github_command_escape(reason)
        );
    }
}

fn github_command_escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

async fn authed_get(client: &Client, base: &str, token: &str, path: &str) -> Result<Response> {
    let response = authed_get_raw(client, base, token, path).await?;
    check_control_status(response).await
}

async fn authed_get_value(client: &Client, base: &str, token: &str, path: &str) -> Result<Value> {
    authed_get(client, base, token, path)
        .await?
        .json()
        .await
        .context("invalid control JSON response")
}

async fn authed_post_value(
    client: &Client,
    base: &str,
    token: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let response = authed_post_raw(client, base, token, path, body).await?;
    check_control_status(response)
        .await?
        .json()
        .await
        .context("invalid control JSON response")
}

fn authed_get_request(
    client: &Client,
    base: &str,
    token: &str,
    path: &str,
) -> reqwest::RequestBuilder {
    client.get(format!("{base}{path}")).bearer_auth(token)
}

fn authed_post_request(
    client: &Client,
    base: &str,
    token: &str,
    path: &str,
    body: Option<&Value>,
) -> reqwest::RequestBuilder {
    let request = client.post(format!("{base}{path}")).bearer_auth(token);
    if let Some(body) = body {
        request.json(body)
    } else {
        request
    }
}

async fn authed_get_raw(client: &Client, base: &str, token: &str, path: &str) -> Result<Response> {
    authed_get_request(client, base, token, path)
        .send()
        .await
        .context("cannot reach LocalView control plane")
}

async fn authed_post_raw(
    client: &Client,
    base: &str,
    token: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Response> {
    authed_post_request(client, base, token, path, body)
        .send()
        .await
        .context("cannot reach LocalView control plane")
}

async fn check_control_status(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if status == StatusCode::UNAUTHORIZED {
        bail!("LocalView control authentication failed");
    }
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    bail!(
        "LocalView control request failed with HTTP {status}: {}",
        compact_status(&value)
    )
}

fn compact_status(value: &Value) -> String {
    value
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| value.get("verdict").and_then(Value::as_str))
        .or_else(|| value.pointer("/result/verdict").and_then(Value::as_str))
        .or_else(|| value.get("completion").and_then(Value::as_str))
        .map(|value| bounded_text(value, 160))
        .unwrap_or_else(|| {
            if value.is_null() {
                "none".into()
            } else {
                "available".into()
            }
        })
}

fn contained_existing_path(project_root: &Path, requested: &Path) -> Result<PathBuf> {
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        project_root.join(requested)
    };
    validate_lexically_contained(project_root, &joined)?;
    let canonical_root =
        std::fs::canonicalize(project_root).context("project root is unavailable")?;
    let canonical = std::fs::canonicalize(&joined).with_context(|| {
        format!(
            "project-contained path is unavailable: {}",
            joined.display()
        )
    })?;
    if !canonical.starts_with(&canonical_root) {
        bail!("path escapes project root");
    }
    Ok(canonical)
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.is_absolute() {
        bail!("fixture cwd must be project-relative");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("fixture cwd may not escape project root");
    }
    Ok(())
}

fn validate_lexically_contained(project_root: &Path, path: &Path) -> Result<()> {
    if !path.is_absolute() || !project_root.is_absolute() {
        bail!("project-bound paths must be absolute after resolution");
    }
    let relative = path
        .strip_prefix(project_root)
        .map_err(|_| anyhow!("path must remain inside project root"))?;
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("path escapes project root");
    }
    Ok(())
}

fn safe_relative_report_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 512
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn fixture_matches_snapshot(fixture: Option<&FixtureSpec>, snapshot: &PageSnapshot) -> bool {
    fixture.is_none_or(|spec| {
        snapshot.route == spec.route
            && snapshot.viewport == (spec.viewport.width, spec.viewport.height)
    })
}

fn headless_state_stable(initial: &PageSnapshot, final_snapshot: &PageSnapshot) -> bool {
    initial.route == final_snapshot.route
        && initial.viewport == final_snapshot.viewport
        && semantic_state_hash(initial) == semantic_state_hash(final_snapshot)
}

fn semantic_state_hash(snapshot: &PageSnapshot) -> String {
    object_hash(&json!({
        "route": snapshot.route,
        "viewport": snapshot.viewport,
        "root": snapshot.root,
    }))
}

fn safe_route(value: &str) -> String {
    bounded_text(
        value
            .split('#')
            .next()
            .unwrap_or(value)
            .split('?')
            .next()
            .unwrap_or(value),
        512,
    )
}

fn valid_sha256_hash(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_owned()
    } else {
        let mut end = max;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

fn generated_at() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{seconds}")
}

fn display_project_relative(project_root: &Path, path: &Path) -> String {
    path.strip_prefix(project_root)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "<project-output>".into())
}

fn default_device_scale_factor() -> f64 {
    1.0
}

fn default_command_timeout_ms() -> u64 {
    DEFAULT_COMMAND_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use localview_diagnostics::DiagnosticIssue;
    use uuid::Uuid;

    fn session(id: Uuid) -> Session {
        serde_json::from_value(json!({
            "id": id,
            "endpoint": {"host":"127.0.0.1","port":5173,"scheme":"http"},
            "classification": {
                "kind":"frontend_dev_server",
                "confidence":1.0,
                "framework":null,
                "title":null,
                "hmr_detected":false,
                "evidence":[]
            },
            "project": {
                "key":"project",
                "display_name":"Project",
                "cwd": std::env::temp_dir().to_string_lossy(),
                "git_root": null,
                "pid": null,
                "command": null
            },
            "status":"active",
            "first_seen":"1970-01-01T00:00:01Z",
            "last_seen":"1970-01-01T00:00:01Z",
            "disconnected_at":null,
            "preview_visible":false
        }))
        .expect("session fixture")
    }

    fn args() -> HeadlessArgs {
        HeadlessArgs {
            session: None,
            analysis: HeadlessAnalysis::Diagnose,
            fixture: None,
            allow_fixture_command: false,
            output_dir: None,
            visual: false,
            chromium: false,
            update_baseline: false,
            design_baseline_hash: None,
            require_baseline_match: false,
            require_verification_pass: false,
            fail_on_heuristic: false,
            deterministic_severity: 3,
            visual_max_changed_ratio: 0.01,
            artifact_budget_mib: 64,
        }
    }

    #[test]
    fn headless_control_requests_carry_bearer_auth() {
        let client = Client::new();
        let request = authed_get_request(
            &client,
            "http://127.0.0.1:45454",
            "test-token",
            "/v1/sessions",
        )
        .build()
        .expect("request");
        assert_eq!(
            request
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-token")
        );

        let post = authed_post_request(
            &client,
            "http://127.0.0.1:45454",
            "test-token",
            "/v1/sessions/00000000-0000-0000-0000-000000000000/proof",
            None,
        )
        .build()
        .expect("post request");
        assert_eq!(
            post.headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-token")
        );
    }

    #[test]
    fn headless_session_resolution_is_exact_and_ambiguous_without_id() {
        assert!(resolve_session(&[], None).is_err());
        let one = session(Uuid::new_v4());
        assert_eq!(
            resolve_session(std::slice::from_ref(&one), None)
                .unwrap()
                .id,
            one.id
        );
        let two = session(Uuid::new_v4());
        assert!(resolve_session(&[one.clone(), two.clone()], None).is_err());
        assert_eq!(
            resolve_session(&[one, two.clone()], Some(two.id))
                .unwrap()
                .id,
            two.id
        );
        assert!(resolve_session(&[two], Some(Uuid::new_v4())).is_err());
    }

    #[test]
    fn fixture_commands_require_explicit_execution_policy() {
        let fixture = FixtureSpec {
            schema_version: 1,
            route: "/".into(),
            viewport: FixtureViewport {
                width: 1280,
                height: 720,
                device_scale_factor: 1.0,
            },
            stable_state: "ready".into(),
            allow_visual: false,
            allow_chromium: false,
            setup: Some(FixtureCommand {
                executable: "node".into(),
                args: vec!["fixture.mjs".into()],
                cwd: None,
                timeout_ms: 1000,
            }),
            cleanup: None,
        };
        assert!(fixture.setup.is_some());
        assert!(!args().allow_fixture_command);
    }

    #[test]
    fn fixture_commands_reject_shells_paths_and_unbounded_timeout() {
        let mut command = FixtureCommand {
            executable: "bash".into(),
            args: vec!["-c".into(), "echo nope".into()],
            cwd: None,
            timeout_ms: 1000,
        };
        assert!(validate_fixture_command(&command).is_err());
        command.executable = "./script".into();
        assert!(validate_fixture_command(&command).is_err());
        command.executable = "node".into();
        command.timeout_ms = MAX_COMMAND_TIMEOUT_MS + 1;
        assert!(validate_fixture_command(&command).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fixture_timeout_is_enforced_without_shell_interpolation() {
        let command = FixtureCommand {
            executable: "sleep".into(),
            args: vec!["1".into()],
            cwd: None,
            timeout_ms: 1,
        };
        let error = run_fixture_command(&std::env::temp_dir(), &command, "test")
            .await
            .expect_err("sleep must time out");
        assert!(error.to_string().contains("timed out"));
    }

    fn page_snapshot(route: &str, viewport: (u32, u32), name: &str) -> PageSnapshot {
        serde_json::from_value(json!({
            "version": 1,
            "route": route,
            "viewport": [viewport.0, viewport.1],
            "root": {
                "reference": "@root",
                "role": "main",
                "name": name,
                "tag": "main",
                "rect": null,
                "interactive": false,
                "attributes": {},
                "source": null,
                "children": []
            },
            "console_errors": [],
            "failed_requests": [],
            "captured_at": "1970-01-01T00:00:01Z"
        }))
        .expect("snapshot fixture")
    }

    #[test]
    fn fixture_route_or_viewport_drift_withholds_baseline_authority() {
        let fixture = FixtureSpec {
            schema_version: 1,
            route: "/".into(),
            viewport: FixtureViewport {
                width: 1280,
                height: 720,
                device_scale_factor: 1.0,
            },
            stable_state: "ready".into(),
            allow_visual: false,
            allow_chromium: false,
            setup: None,
            cleanup: None,
        };
        assert!(fixture_matches_snapshot(
            Some(&fixture),
            &page_snapshot("/", (1280, 720), "ready")
        ));
        assert!(!fixture_matches_snapshot(
            Some(&fixture),
            &page_snapshot("/other", (1280, 720), "ready")
        ));
        assert!(!fixture_matches_snapshot(
            Some(&fixture),
            &page_snapshot("/", (1024, 768), "ready")
        ));
    }

    #[test]
    fn route_viewport_or_semantic_drift_withholds_stable_state() {
        let initial = page_snapshot("/", (1280, 720), "ready");
        assert!(headless_state_stable(
            &initial,
            &page_snapshot("/", (1280, 720), "ready")
        ));
        assert!(!headless_state_stable(
            &initial,
            &page_snapshot("/other", (1280, 720), "ready")
        ));
        assert!(!headless_state_stable(
            &initial,
            &page_snapshot("/", (1024, 768), "ready")
        ));
        assert!(!headless_state_stable(
            &initial,
            &page_snapshot("/", (1280, 720), "changed")
        ));
    }

    #[test]
    fn git_annotation_covers_clean_dirty_and_unavailable_states() {
        let diagnostics = DiagnosticReport::default();
        let clean = git_project_state_from_value(
            &json!({
                "commit": "abc",
                "branch": "main",
                "dirty_files": [],
                "working_tree_id": "wt:abc"
            }),
            &diagnostics,
        );
        assert_eq!(clean.annotation.dirty, Some(false));
        assert_eq!(clean.annotation.revision.as_deref(), Some("abc"));

        let dirty = git_project_state_from_value(
            &json!({
                "commit": "abc",
                "branch": "feature",
                "dirty_files": ["src/app.rs", "/home/user/secret.rs", "../escape.rs"],
                "working_tree_id": "wt:abc+dirty.3"
            }),
            &diagnostics,
        );
        assert_eq!(dirty.annotation.dirty, Some(true));
        assert_eq!(dirty.annotation.changed_files, vec!["src/app.rs"]);

        let unavailable = unavailable_git_project_state();
        assert!(!unavailable.annotation.available);
        assert_eq!(
            unavailable.annotation.unavailable_reason.as_deref(),
            Some("git unavailable")
        );
    }

    #[test]
    fn fixture_state_identity_is_stable() {
        let input = StateIdentityInput {
            project_key: "project",
            route: "/checkout",
            viewport: (1280, 720),
            fixture_hash: Some("sha256:fixture"),
            stable_state: Some("ready"),
        };
        assert_eq!(object_hash(&input), object_hash(&input));
    }

    #[test]
    fn fixture_policy_disables_visual_and_chromium_explicitly() {
        let fixture = FixtureSpec {
            schema_version: 1,
            route: "/".into(),
            viewport: FixtureViewport {
                width: 1280,
                height: 720,
                device_scale_factor: 1.0,
            },
            stable_state: "ready".into(),
            allow_visual: false,
            allow_chromium: false,
            setup: None,
            cleanup: None,
        };
        assert!(!visual_permitted(Some(&fixture)));
        assert!(!chromium_permitted(Some(&fixture)));
    }

    #[test]
    fn visual_capture_retries_only_for_newly_uncomparable_baseline() {
        assert!(visual_result_needs_baseline_retry(&json!({
            "result": {
                "verdict": "inconclusive",
                "reason": "visual assertion has no comparable baseline"
            }
        })));
        assert!(!visual_result_needs_baseline_retry(&json!({
            "result": {
                "verdict": "pass",
                "reason": "changed ratio is within unchanged limit"
            }
        })));
        assert!(!visual_result_needs_baseline_retry(&json!({
            "result": {
                "verdict": "inconclusive",
                "reason": "different inconclusive cause"
            }
        })));
    }

    #[test]
    fn resource_denial_and_unavailable_visual_are_classified_separately() {
        assert_eq!(
            classify_control_status(
                StatusCode::TOO_MANY_REQUESTS,
                &json!({"error":"resource_governor_denied"})
            ),
            BoundedRequestDisposition::ResourceDenied
        );
        assert_eq!(
            classify_control_status(
                StatusCode::BAD_GATEWAY,
                &json!({"error":"native_visual_diff_failed"})
            ),
            BoundedRequestDisposition::Unavailable("native_visual_diff_failed".into())
        );
    }

    #[tokio::test]
    async fn missing_retained_baseline_requires_explicit_update() {
        let root = std::env::temp_dir().join(format!(
            "localview-wave8-missing-baseline-{}",
            Uuid::new_v4()
        ));
        let artifact_root = root.join("artifacts");
        tokio::fs::create_dir_all(&artifact_root).await.unwrap();
        let candidate = BaselineEnvelope {
            schema_version: 1,
            state_identity: "sha256:state".into(),
            route: "/".into(),
            viewport: (1280, 720),
            evidence_hashes: vec!["sha256:evidence".into()],
            design_baseline_hash: None,
            created_revision: Some("abc".into()),
            provenance: BTreeMap::new(),
        }
        .normalized();

        let mut store = ArtifactStore::open(&artifact_root, 1024 * 1024)
            .await
            .unwrap();
        let (created, _) =
            compare_and_retain_baseline(&root, &artifact_root, &mut store, &candidate, true, false)
                .await
                .unwrap();
        assert_eq!(created.status, BaselineComparisonStatus::Created);

        let index = load_baseline_index(&root.join("baseline-index.json"))
            .await
            .unwrap();
        let locator = index.states.get(&candidate.state_identity).unwrap();
        tokio::fs::remove_file(artifact_root.join(&locator.storage_id))
            .await
            .unwrap();

        let mut reopened = ArtifactStore::open(&artifact_root, 1024 * 1024)
            .await
            .unwrap();
        let (comparison, replacement) = compare_and_retain_baseline(
            &root,
            &artifact_root,
            &mut reopened,
            &candidate,
            true,
            false,
        )
        .await
        .unwrap();
        assert_eq!(comparison.status, BaselineComparisonStatus::Incompatible);
        assert!(replacement.is_none());

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn physical_storage_id_is_not_confused_with_canonical_hash() {
        assert!(valid_physical_artifact_id("lv-0123456789abcdef"));
        assert!(!valid_physical_artifact_id("sha256:0123456789abcdef"));
    }

    #[test]
    fn baseline_evidence_selection_is_run_local_not_history_wide() {
        let ids = baseline_evidence_ids(
            &json!({
                "fresh_evidence_ids": ["ev_semantic", "ev_layout"]
            }),
            &json!({"evidence_id": "ev_visual", "result": {"verdict": "pass"}}),
            &json!({
                "steps": [{
                    "execution": {
                        "kind": "chromium_compatibility",
                        "evidence_id": "ev_chromium"
                    }
                }]
            }),
        );
        assert_eq!(
            ids,
            std::collections::BTreeSet::from([
                "ev_chromium".to_owned(),
                "ev_layout".to_owned(),
                "ev_semantic".to_owned(),
                "ev_visual".to_owned(),
            ])
        );

        let history = json!([
            {"id":"ev_old","kind":"semantic","secret_taint":false,"payload":{"state":"old"},"provenance":{"source":"observer","revision":"abc"}},
            {"id":"ev_semantic","kind":"semantic","secret_taint":false,"payload":{"state":"current"},"provenance":{"source":"observer","revision":"abc"}}
        ]);
        let summary = summarize_evidence(&history, &ids);
        assert_eq!(summary.hashes.len(), 2);
        assert_eq!(summary.baseline_hashes.len(), 1);
    }

    #[test]
    fn secret_tainted_evidence_is_excluded_from_report_hash_inputs() {
        let value = json!([
            {"id":"ev_secret","kind":"semantic","secret_taint":true,"payload":{"token":"secret"}},
            {"id":"ev_safe","kind":"layout","secret_taint":false,"uncertainty":"observed","provenance":{"source":"native","revision":"abc"}}
        ]);
        let summary = summarize_evidence(
            &value,
            &std::collections::BTreeSet::from(["ev_safe".to_owned()]),
        );
        assert_eq!(summary.ids, vec!["ev_safe"]);
        assert_eq!(summary.hashes.len(), 1);
        assert_eq!(summary.baseline_hashes.len(), 1);
    }

    #[test]
    fn secret_paths_are_redacted_from_diagnostics() {
        let root = std::env::temp_dir().join("wave8-secret-root");
        let root_text = root.to_string_lossy().into_owned();
        let report = DiagnosticReport {
            issues: vec![DiagnosticIssue {
                category: "source".into(),
                code: "path".into(),
                message: format!("failure at {root_text}/src/main.rs"),
                severity: 3,
                confidence: 100,
                class: DiagnosticClass::Deterministic,
                refs: vec![format!("{root_text}/src/main.rs")],
                evidence: Some(format!("{root_text}/secret.txt")),
            }],
            deterministic: 1,
            heuristic: 0,
            subjective: 0,
        };
        let safe = sanitize_diagnostics(report, &root);
        let encoded = serde_json::to_string(&safe).unwrap();
        assert!(!encoded.contains(&root_text));
        assert!(encoded.contains("<project>"));
    }

    #[test]
    fn route_and_output_paths_cannot_escape_project() {
        assert!(validate_relative_path(Path::new("fixtures/app")).is_ok());
        assert!(validate_relative_path(Path::new("../outside")).is_err());
    }

    #[test]
    fn visual_fail_is_a_hard_gate_and_inconclusive_is_not() {
        let diagnostics = DiagnosticReport::default();
        assert_eq!(
            evaluate_exit_policy(
                &args(),
                &diagnostics,
                "pass",
                BaselineComparisonStatus::Match,
                true,
                false,
            ),
            (ReportStatus::Failed, EXIT_HARD_FAILURE)
        );
        assert_eq!(
            evaluate_exit_policy(
                &args(),
                &diagnostics,
                "pass",
                BaselineComparisonStatus::Match,
                false,
                true,
            ),
            (ReportStatus::Inconclusive, EXIT_INCONCLUSIVE)
        );
    }

    #[test]
    fn chromium_request_requires_a_real_compatibility_receipt() {
        assert!(chromium_cycle_executed(&json!({
            "steps": [{
                "execution": {
                    "kind": "chromium_compatibility",
                    "exit_code": 0,
                    "evidence_id": "ev_chromium"
                }
            }]
        })));
        assert!(!chromium_cycle_executed(&json!({
            "completion": "no_op",
            "steps": [{
                "execution": {"kind": "semantic_snapshot"}
            }]
        })));
    }

    #[test]
    fn heuristic_only_findings_do_not_fail_default_policy() {
        let diagnostics = DiagnosticReport {
            issues: vec![DiagnosticIssue {
                category: "layout".into(),
                code: "heuristic".into(),
                message: "heuristic only".into(),
                severity: 3,
                confidence: 90,
                class: DiagnosticClass::Heuristic,
                refs: Vec::new(),
                evidence: None,
            }],
            deterministic: 0,
            heuristic: 1,
            subjective: 0,
        };
        assert_eq!(
            evaluate_exit_policy(
                &args(),
                &diagnostics,
                "pass",
                BaselineComparisonStatus::Match,
                false,
                false,
            ),
            (ReportStatus::Passed, EXIT_PASS)
        );
    }

    #[test]
    fn deterministic_hard_failure_has_stable_exit_code() {
        let diagnostics = DiagnosticReport {
            issues: vec![DiagnosticIssue {
                category: "layout".into(),
                code: "overflow".into(),
                message: "deterministic failure".into(),
                severity: 3,
                confidence: 100,
                class: DiagnosticClass::Deterministic,
                refs: Vec::new(),
                evidence: None,
            }],
            deterministic: 1,
            heuristic: 0,
            subjective: 0,
        };
        assert_eq!(
            evaluate_exit_policy(
                &args(),
                &diagnostics,
                "pass",
                BaselineComparisonStatus::Match,
                false,
                false,
            ),
            (ReportStatus::Failed, EXIT_HARD_FAILURE)
        );
    }
}
