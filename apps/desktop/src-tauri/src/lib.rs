#![forbid(unsafe_code)]

mod native_executor_worker;
mod trusted_ai;
mod trusted_fix;
mod trusted_verify;
pub mod visual_capture;
pub mod workspace_surface;

use std::{
    ffi::OsString,
    io::{BufRead, BufReader},
    path::{Component, Path, PathBuf},
    process::Command,
};

use localview_instrumentation::{bootstrap_script, InstrumentationConfig};
use localview_live_bridge::{
    ActionCancellationSignal, BridgeAction, BridgeActionKind, BridgeActionResult, IngestReport,
    NetworkFaultControlRequest, NetworkFaultControlResult, ObserverBatch, ObserverEvent,
    PrivateBridgeAction,
};
use localview_protocol::{Health, PageSnapshot, SemanticNode, Session, SessionId, SourceLocation};
use serde::{Deserialize, Serialize};
use tauri::menu::MenuBuilder;
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Serialize)]
struct DashboardState {
    health: Health,
    sessions: Vec<Session>,
    engine: EngineInfo,
    capabilities: Vec<&'static str>,
    workspace_surface: workspace_surface::WorkspaceSurfaceSupport,
}

#[derive(Debug, Serialize)]
struct EngineInfo {
    native: &'static str,
    tier3: &'static str,
}

#[derive(Debug, Serialize)]
struct LiveSessionState {
    observer: Vec<ObserverEvent>,
    action_results: Vec<BridgeActionResult>,
}

const MEASURE_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2_500);
const MEASURE_RESULT_POLL: std::time::Duration = std::time::Duration::from_millis(50);
const MAX_MEASURE_REFERENCE_BYTES: usize = 64;
const MAX_MEASURE_CSS_DIMENSION: f64 = 100_000.0;
const MAX_MEASURE_ABS_COORDINATE: f64 = 1_000_000.0;
const MEASURE_DIMENSION_TOLERANCE: f64 = 0.2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MeasureRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ElementMeasureReceipt {
    reference: String,
    rect: MeasureRect,
    document_rect: MeasureRect,
    viewport_css_width: f64,
    viewport_css_height: f64,
    route: String,
    measured_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct MeasureViewportPayload {
    width: f64,
    height: f64,
}

#[derive(Debug, Deserialize)]
struct MeasurePayload {
    reference: String,
    rect: MeasureRect,
    document_rect: MeasureRect,
    viewport: MeasureViewportPayload,
    route: String,
}

const MAX_SOURCE_REFERENCE_BYTES: usize = 64;
const MAX_SOURCE_FILE_BYTES: usize = 512;
const MAX_SOURCE_LINE: u32 = 10_000_000;
const MAX_SOURCE_COLUMN: u32 = 100_000;
const MAX_SOURCE_VERIFY_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
struct TrustedSourceTarget {
    session_id: SessionId,
    reference: String,
    project_root: PathBuf,
    canonical_file: PathBuf,
    project_relative_file: String,
    line: u32,
    column: Option<u32>,
    snapshot_version: u64,
    canonical_route: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum SourceOpenLauncher {
    MacOpen,
    LinuxXdgOpen,
    WindowsFileProtocolHandler,
}

#[derive(Debug, Clone)]
struct TrustedSourceLaunchPlan {
    launcher: SourceOpenLauncher,
    program: &'static str,
    args: Vec<OsString>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HumanSourceOpenReceipt {
    reference: String,
    display_file: String,
    line: u32,
    column: Option<u32>,
    launcher: SourceOpenLauncher,
    snapshot_version: u64,
}

fn validate_source_reference(reference: &str) -> Result<(), String> {
    if reference.len() > MAX_SOURCE_REFERENCE_BYTES {
        return Err("trusted source element reference exceeds the safety bound".into());
    }
    let Some(hash) = reference.strip_prefix("@e") else {
        return Err("trusted source requires a LocalView element reference".into());
    };
    if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("trusted source element reference is malformed".into());
    }
    Ok(())
}

fn find_source_for_reference<'a>(
    node: &'a SemanticNode,
    reference: &str,
    found: &mut Option<&'a SourceLocation>,
    matches: &mut usize,
) {
    if node.reference == reference {
        *matches += 1;
        if found.is_none() {
            *found = node.source.as_ref();
        }
    }
    for child in &node.children {
        find_source_for_reference(child, reference, found, matches);
    }
}

fn resolve_snapshot_source<'a>(
    snapshot: &'a PageSnapshot,
    reference: &str,
) -> Result<&'a SourceLocation, String> {
    let mut found = None;
    let mut matches = 0usize;
    find_source_for_reference(&snapshot.root, reference, &mut found, &mut matches);
    if matches == 0 {
        return Err("trusted source selection is no longer available".into());
    }
    if matches != 1 {
        return Err("trusted source selection is ambiguous".into());
    }
    found.ok_or_else(|| "trusted source mapping is unavailable".to_string())
}

fn validate_relative_source_path(file: &str) -> Result<&Path, String> {
    if file.is_empty()
        || file.len() > MAX_SOURCE_FILE_BYTES
        || file.contains('\0')
        || file.contains(':')
        || file.contains("://")
        || file.starts_with("\\\\")
    {
        return Err("trusted source path is invalid".into());
    }
    #[cfg(not(windows))]
    if file.contains('\\') {
        return Err("trusted source path uses a non-native separator".into());
    }

    let path = Path::new(file);
    if path.is_absolute() || path.has_root() {
        return Err("trusted source path must be project relative".into());
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir => {
                return Err("trusted source path traversal is not allowed".into());
            }
            Component::Prefix(_) => {
                return Err("trusted source path prefix is not allowed".into());
            }
        }
    }
    Ok(path)
}

fn validate_source_line_exists(canonical_file: &Path, line: u32) -> Result<(), String> {
    let metadata = std::fs::metadata(canonical_file)
        .map_err(|_| "trusted source file is unavailable".to_string())?;
    if metadata.len() > MAX_SOURCE_VERIFY_BYTES {
        return Err("trusted source file exceeds verification bound".into());
    }

    let file = std::fs::File::open(canonical_file)
        .map_err(|_| "trusted source file is unavailable".to_string())?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();

    for _ in 0..line {
        buffer.clear();
        let read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|_| "trusted source file is unavailable".to_string())?;
        if read == 0 {
            return Err("trusted source line is unavailable".into());
        }
    }

    Ok(())
}

fn resolve_trusted_source_target(
    session_id: SessionId,
    reference: &str,
    project_root: &str,
    source: &SourceLocation,
    snapshot_version: u64,
    canonical_route: &str,
) -> Result<TrustedSourceTarget, String> {
    validate_source_reference(reference)?;
    if source.line == 0 || source.line > MAX_SOURCE_LINE {
        return Err("trusted source line is outside the safety bound".into());
    }
    if source
        .column
        .is_some_and(|column| column == 0 || column > MAX_SOURCE_COLUMN)
    {
        return Err("trusted source column is outside the safety bound".into());
    }
    let relative = validate_relative_source_path(&source.file)?;
    let canonical_project_root = std::fs::canonicalize(project_root)
        .map_err(|_| "trusted source project root is unavailable".to_string())?;
    if !canonical_project_root.is_dir() {
        return Err("trusted source project root is unavailable".into());
    }
    let canonical_file = std::fs::canonicalize(canonical_project_root.join(relative))
        .map_err(|_| "trusted source file is unavailable".to_string())?;
    if !canonical_file.starts_with(&canonical_project_root) {
        return Err("trusted source outside project".into());
    }
    let metadata = std::fs::metadata(&canonical_file)
        .map_err(|_| "trusted source file is unavailable".to_string())?;
    if !metadata.is_file() {
        return Err("trusted source target is not a regular file".into());
    }
    validate_source_line_exists(&canonical_file, source.line)?;
    let project_relative_file = canonical_file
        .strip_prefix(&canonical_project_root)
        .map_err(|_| "trusted source outside project".to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    if project_relative_file.is_empty() {
        return Err("trusted source file is unavailable".into());
    }

    Ok(TrustedSourceTarget {
        session_id,
        reference: reference.to_owned(),
        project_root: canonical_project_root,
        canonical_file,
        project_relative_file,
        line: source.line,
        column: source.column,
        snapshot_version,
        canonical_route: canonical_route.to_owned(),
    })
}

fn trusted_source_launch_plan(
    target: &TrustedSourceTarget,
) -> Result<TrustedSourceLaunchPlan, String> {
    let _ = (
        target.session_id,
        &target.reference,
        &target.project_root,
        &target.canonical_route,
    );

    #[cfg(target_os = "macos")]
    {
        return Ok(TrustedSourceLaunchPlan {
            launcher: SourceOpenLauncher::MacOpen,
            program: "/usr/bin/open",
            args: vec![target.canonical_file.clone().into_os_string()],
        });
    }

    #[cfg(target_os = "linux")]
    {
        return Ok(TrustedSourceLaunchPlan {
            launcher: SourceOpenLauncher::LinuxXdgOpen,
            program: "xdg-open",
            args: vec![target.canonical_file.clone().into_os_string()],
        });
    }

    #[cfg(target_os = "windows")]
    {
        return Ok(TrustedSourceLaunchPlan {
            launcher: SourceOpenLauncher::WindowsFileProtocolHandler,
            program: "rundll32.exe",
            args: vec![
                OsString::from("url.dll,FileProtocolHandler"),
                target.canonical_file.clone().into_os_string(),
            ],
        });
    }

    #[allow(unreachable_code)]
    Err("trusted source launcher unavailable".into())
}

fn launch_trusted_source_with<F>(
    target: &TrustedSourceTarget,
    spawn: F,
) -> Result<SourceOpenLauncher, String>
where
    F: FnOnce(&TrustedSourceLaunchPlan) -> Result<(), ()>,
{
    let plan = trusted_source_launch_plan(target)?;
    spawn(&plan).map_err(|_| "trusted source launcher unavailable".to_string())?;
    Ok(plan.launcher)
}

fn launch_trusted_source(target: &TrustedSourceTarget) -> Result<SourceOpenLauncher, String> {
    launch_trusted_source_with(target, |plan| {
        Command::new(plan.program)
            .args(&plan.args)
            .spawn()
            .map(|_| ())
            .map_err(|_| ())
    })
}

#[tauri::command]
async fn dashboard_state() -> Result<DashboardState, String> {
    let client = control_client()?;
    let health = client
        .get("http://127.0.0.1:45454/health")
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Health>()
        .await
        .map_err(err)?;
    let token = read_token().await?;
    let sessions = client
        .get("http://127.0.0.1:45454/v1/sessions")
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<Session>>()
        .await
        .map_err(err)?;
    Ok(DashboardState {
        health,
        sessions,
        engine: EngineInfo {
            native: native_engine(),
            tier3: "Chromium / Playwright on demand",
        },
        capabilities: vec![
            "Discovery",
            "Sessions",
            "Observation",
            "Instrumentation",
            "Live Bridge",
            "Semantic Diff",
            "Layout",
            "Visual Diff",
            "Responsive",
            "Source Map",
            "Source Graph",
            "Network",
            "Console",
            "A11y",
            "Performance",
            "Capture",
            "Flow Replay",
            "Design Grammar",
            "Diagnostics",
            "Reports",
            "Token Budget",
            "Evidence",
            "Causal Runtime",
            "Contracts",
            "State Space",
            "Counterfactual",
            "Verification",
            "MCP",
        ],
        workspace_surface: workspace_surface::workspace_surface_support(),
    })
}

#[tauri::command]
async fn live_session_state(session_id: SessionId) -> Result<LiveSessionState, String> {
    let token = read_token().await?;
    let client = control_client()?;
    let observer = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/observer/recent"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<ObserverEvent>>()
        .await
        .map_err(err)?;
    let action_results = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/results"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<BridgeActionResult>>()
        .await
        .map_err(err)?;
    Ok(LiveSessionState {
        observer,
        action_results,
    })
}

#[tauri::command]
async fn action_correlation(
    session_id: SessionId,
    action_id: uuid::Uuid,
) -> Result<Option<serde_json::Value>, String> {
    let token = read_token().await?;
    let response = control_client()?
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/{action_id}/correlation"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?;
    if matches!(
        response.status(),
        reqwest::StatusCode::CONFLICT | reqwest::StatusCode::NOT_FOUND
    ) {
        return Ok(None);
    }
    let value = response
        .error_for_status()
        .map_err(err)?
        .json::<serde_json::Value>()
        .await
        .map_err(err)?;
    Ok(Some(value))
}

#[tauri::command]
fn ai_provider_capability() -> Result<trusted_ai::AiProviderCapability, String> {
    Ok(trusted_ai::provider_capability_from_env())
}

#[tauri::command]
async fn ask_ai_about_selection(
    app: tauri::AppHandle,
    session_id: SessionId,
    reference: String,
    question: String,
) -> Result<trusted_ai::HumanAskAiReceipt, String> {
    trusted_ai::validate_reference(&reference)?;
    let question = trusted_ai::validate_question(&question)?;
    let pre_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;

    let token = read_token().await?;
    let client = control_client()?;

    let session = client
        .get(format!("http://127.0.0.1:45454/v1/sessions/{session_id}"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| "trusted AI runtime unavailable".to_string())?
        .error_for_status()
        .map_err(|_| "trusted AI session is unavailable".to_string())?
        .json::<Session>()
        .await
        .map_err(|_| "trusted AI session is unavailable".to_string())?;

    let snapshot = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/semantic-snapshot/fresh"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| "trusted AI runtime unavailable".to_string())?
        .error_for_status()
        .map_err(|_| "trusted AI context is unavailable".to_string())?
        .json::<PageSnapshot>()
        .await
        .map_err(|_| "trusted AI context is unavailable".to_string())?;

    let snapshot_route = visual_capture::canonical_visual_diff_route(&snapshot.route)?;
    if snapshot_route != pre_route {
        return Err("trusted AI route changed before context resolution".into());
    }

    let context = trusted_ai::build_trusted_ai_context(&session, &snapshot, &reference)?;

    let post_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    if post_route != pre_route {
        return Err("trusted AI route changed while context was being prepared".into());
    }

    let provider = trusted_ai::provider_config_from_env()
        .map_err(|_| "trusted AI provider unavailable".to_string())?;
    let provider_answer =
        trusted_ai::ask_with_provider(&client, &provider, &context, &question).await?;

    Ok(trusted_ai::HumanAskAiReceipt {
        reference,
        answer: provider_answer.answer,
        provider_label: provider_answer.provider_label,
        context_version: context.context_version,
        snapshot_version: context.snapshot_version,
        completed_at_unix_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
    })
}

#[tauri::command]
fn ai_fix_capability() -> Result<trusted_fix::AiFixCapability, String> {
    Ok(trusted_fix::fix_capability_from_env())
}

#[tauri::command]
async fn prepare_fix_proposal(
    app: tauri::AppHandle,
    store: tauri::State<'_, trusted_fix::FixProposalStore>,
    session_id: SessionId,
    reference: String,
    instruction: String,
) -> Result<trusted_fix::HumanFixProposalReceipt, String> {
    validate_source_reference(&reference)?;
    let instruction = trusted_fix::validate_fix_instruction(&instruction)?;
    let capability = trusted_fix::fix_capability_from_env();
    if !capability.available {
        return Err("trusted Fix provider unavailable".into());
    }

    let pre_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    let token = read_token().await?;
    let client = control_client()?;

    let session = client
        .get(format!("http://127.0.0.1:45454/v1/sessions/{session_id}"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| "trusted Fix runtime unavailable".to_string())?
        .error_for_status()
        .map_err(|_| "trusted Fix session is unavailable".to_string())?
        .json::<Session>()
        .await
        .map_err(|_| "trusted Fix session is unavailable".to_string())?;

    let snapshot = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/semantic-snapshot/fresh"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| "trusted Fix runtime unavailable".to_string())?
        .error_for_status()
        .map_err(|_| "trusted Fix source mapping is unavailable".to_string())?
        .json::<PageSnapshot>()
        .await
        .map_err(|_| "trusted Fix source mapping is unavailable".to_string())?;

    let snapshot_route = visual_capture::canonical_visual_diff_route(&snapshot.route)?;
    if snapshot_route != pre_route {
        return Err("trusted Fix route changed before proposal resolution".into());
    }

    let source = resolve_snapshot_source(&snapshot, &reference)?;
    let project_root = session
        .project
        .git_root
        .as_deref()
        .or(session.project.cwd.as_deref())
        .ok_or_else(|| "trusted Fix project root is unavailable".to_string())?;
    let target = resolve_trusted_source_target(
        session_id,
        &reference,
        project_root,
        source,
        snapshot.version,
        &pre_route,
    )?;

    let preimage = trusted_fix::validate_fix_source_policy(&target)?;
    let excerpt = trusted_fix::build_source_excerpt(&target, &preimage)?;
    let context = trusted_ai::build_trusted_ai_context(&session, &snapshot, &reference)?;

    let route_before_provider =
        visual_capture::managed_surface_canonical_route(&app, session_id)?;
    if route_before_provider != pre_route {
        return Err("trusted Fix route changed while proposal context was prepared".into());
    }

    let provider = trusted_ai::provider_config_from_env()
        .map_err(|_| "trusted Fix provider unavailable".to_string())?;
    let (summary, edit, provider_label) = trusted_fix::request_fix_proposal(
        &client,
        &provider,
        &context,
        &excerpt,
        &instruction,
    )
    .await?;

    let post_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    if post_route != pre_route {
        return Err("trusted Fix route changed while proposal was generated".into());
    }

    let current_preimage = trusted_fix::validate_fix_source_policy(&target)?;
    if current_preimage != preimage {
        return Err("trusted Fix source changed while proposal was generated".into());
    }

    let postimage = trusted_fix::build_fix_postimage(&preimage, &excerpt, &edit)?;
    let diff = trusted_fix::build_fix_diff(
        &target.project_relative_file,
        &preimage,
        &postimage,
        &edit,
    )?;
    let proposal = trusted_fix::new_proposal_record(
        &target,
        preimage,
        postimage,
        &edit,
        instruction.clone(),
        summary,
        diff,
        provider_label,
    );
    let receipt = trusted_fix::proposal_receipt(&proposal);
    store.insert(proposal)?;
    Ok(receipt)
}

#[tauri::command]
async fn apply_fix_proposal(
    app: tauri::AppHandle,
    store: tauri::State<'_, trusted_fix::FixProposalStore>,
    visual_state: tauri::State<'_, visual_capture::VisualCaptureState>,
    verification_store: tauri::State<'_, trusted_verify::VerificationStore>,
    proposal_id: String,
) -> Result<trusted_fix::HumanApplyFixReceipt, String> {
    let proposal = store.begin_apply(&proposal_id)?;
    let gate = store.apply_gate_for(&proposal.canonical_file)?;
    let _guard = gate.lock().await;

    let result = async {
        let pre_route =
            visual_capture::managed_surface_canonical_route(&app, proposal.session_id)?;
        if pre_route != proposal.canonical_route {
            return Err("trusted Fix route changed since proposal".to_string());
        }

        let token = read_token().await?;
        let client = control_client()?;
        let session = client
            .get(format!(
                "http://127.0.0.1:45454/v1/sessions/{}",
                proposal.session_id
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "trusted Fix runtime unavailable".to_string())?
            .error_for_status()
            .map_err(|_| "trusted Fix session is unavailable".to_string())?
            .json::<Session>()
            .await
            .map_err(|_| "trusted Fix session is unavailable".to_string())?;

        let snapshot = client
            .get(format!(
                "http://127.0.0.1:45454/v1/sessions/{}/semantic-snapshot/fresh",
                proposal.session_id
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "trusted Fix runtime unavailable".to_string())?
            .error_for_status()
            .map_err(|_| "trusted Fix source mapping is unavailable".to_string())?
            .json::<PageSnapshot>()
            .await
            .map_err(|_| "trusted Fix source mapping is unavailable".to_string())?;

        let snapshot_route = visual_capture::canonical_visual_diff_route(&snapshot.route)?;
        if snapshot_route != proposal.canonical_route {
            return Err("trusted Fix route changed since proposal".to_string());
        }

        let source = resolve_snapshot_source(&snapshot, &proposal.reference)?;
        let project_root = session
            .project
            .git_root
            .as_deref()
            .or(session.project.cwd.as_deref())
            .ok_or_else(|| "trusted Fix project root is unavailable".to_string())?;
        let current_target = resolve_trusted_source_target(
            proposal.session_id,
            &proposal.reference,
            project_root,
            source,
            snapshot.version,
            &pre_route,
        )?;

        if current_target.canonical_file != proposal.canonical_file
            || current_target.project_root != proposal.project_root
            || current_target.project_relative_file != proposal.display_file
            || current_target.line != proposal.source_line
        {
            return Err("trusted Fix source mapping changed since proposal".into());
        }

        let current_bytes = trusted_fix::validate_fix_source_policy(&current_target)?;
        if current_bytes != proposal.preimage {
            return Err("trusted Fix source changed since proposal".into());
        }

        let post_route =
            visual_capture::managed_surface_canonical_route(&app, proposal.session_id)?;
        if post_route != proposal.canonical_route {
            return Err("trusted Fix route changed since proposal".into());
        }

        let semantic_before =
            trusted_verify::build_semantic_baseline(&session, &snapshot, &proposal.reference)?;
        let visual_before = match visual_capture::capture_verification_baseline(
            app.clone(),
            &visual_state,
            proposal.session_id,
        )
        .await
        {
            Ok(frame) => {
                let frame_route = visual_capture::canonical_visual_diff_route(&frame.route)?;
                if frame_route != proposal.canonical_route {
                    return Err(
                        "trusted Fix route changed while verification baseline was captured".into(),
                    );
                }
                Some(trusted_verify::VerifyVisualBaseline {
                    png: std::sync::Arc::new(frame.png),
                    viewport: frame.viewport,
                    pixel_width: frame.pixel_width,
                    pixel_height: frame.pixel_height,
                    target_rect: semantic_before.selected.rect.clone(),
                    captured_at_unix_ms: frame.captured_at_unix_ms,
                })
            }
            Err(_) => None,
        };

        let pre_write_route =
            visual_capture::managed_surface_canonical_route(&app, proposal.session_id)?;
        if pre_write_route != proposal.canonical_route {
            return Err("trusted Fix route changed before apply".into());
        }

        let (verification_id, verification_scope) = trusted_verify::mint_verification_baseline(
            &verification_store,
            &proposal.proposal_id,
            &session,
            &snapshot,
            &proposal.reference,
            &proposal.canonical_route,
            proposal.canonical_file.clone(),
            proposal.project_root.clone(),
            proposal.display_file.clone(),
            proposal.source_line,
            proposal.postimage.clone(),
            proposal.instruction.clone(),
            visual_before,
        )?;

        if let Err(error) = trusted_fix::apply_fix_transaction(
            &current_target.canonical_file,
            &proposal.preimage,
            &proposal.postimage,
        ) {
            let _ = verification_store.discard_verification(&verification_id);
            return Err(error);
        }

        Ok::<trusted_fix::HumanApplyFixReceipt, String>(
            trusted_fix::HumanApplyFixReceipt {
                proposal_id: proposal.proposal_id.clone(),
                reference: proposal.reference.clone(),
                display_file: proposal.display_file.clone(),
                applied: true,
                changed_start_line: proposal.changed_start_line,
                changed_end_line: proposal.changed_end_line,
                verification_id,
                verification_scope,
                applied_at_unix_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            },
        )
    }
    .await;

    match result {
        Ok(receipt) => {
            store.complete_apply(&proposal_id)?;
            Ok(receipt)
        }
        Err(error) => {
            let _ = store.invalidate(&proposal_id);
            Err(error)
        }
    }
}

#[tauri::command]
fn discard_fix_proposal(
    store: tauri::State<'_, trusted_fix::FixProposalStore>,
    proposal_id: String,
) -> Result<(), String> {
    store.discard(&proposal_id)
}

#[tauri::command]
async fn verify_fix_change(
    app: tauri::AppHandle,
    visual_state: tauri::State<'_, visual_capture::VisualCaptureState>,
    verification_store: tauri::State<'_, trusted_verify::VerificationStore>,
    verification_id: String,
) -> Result<trusted_verify::HumanVerifyChangeReceipt, String> {
    let record = verification_store.begin_verify(&verification_id)?;
    if record.semantic_before.context_version != trusted_verify::VERIFY_CONTEXT_VERSION {
        let _ = verification_store.invalidate(&verification_id);
        return Err("trusted Verify context version is unsupported".into());
    }
    let result = match tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let pre_route =
            visual_capture::managed_surface_canonical_route(&app, record.session_id)?;
        if pre_route != record.canonical_route {
            return Err("trusted Verify route changed since Apply".to_string());
        }

        visual_capture::wait_for_verification_settle(record.session_id).await?;

        let token = read_token().await?;
        let client = control_client()?;
        let session = client
            .get(format!(
                "http://127.0.0.1:45454/v1/sessions/{}",
                record.session_id
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "trusted Verify runtime unavailable".to_string())?
            .error_for_status()
            .map_err(|_| "trusted Verify session is unavailable".to_string())?
            .json::<Session>()
            .await
            .map_err(|_| "trusted Verify session is unavailable".to_string())?;

        let snapshot = client
            .get(format!(
                "http://127.0.0.1:45454/v1/sessions/{}/semantic-snapshot/fresh",
                record.session_id
            ))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| "trusted Verify runtime unavailable".to_string())?
            .error_for_status()
            .map_err(|_| "trusted Verify target is unavailable".to_string())?
            .json::<PageSnapshot>()
            .await
            .map_err(|_| "trusted Verify target is unavailable".to_string())?;

        let snapshot_route = visual_capture::canonical_visual_diff_route(&snapshot.route)?;
        if snapshot_route != record.canonical_route {
            return Err("trusted Verify route changed since Apply".into());
        }

        let source = resolve_snapshot_source(&snapshot, &record.reference)?;
        let project_root = session
            .project
            .git_root
            .as_deref()
            .or(session.project.cwd.as_deref())
            .ok_or_else(|| "trusted Verify project root is unavailable".to_string())?;
        let target = resolve_trusted_source_target(
            record.session_id,
            &record.reference,
            project_root,
            source,
            snapshot.version,
            &pre_route,
        )?;
        if target.canonical_file != record.canonical_file
            || target.project_root != record.project_root
            || target.project_relative_file != record.display_file
            || target.line != record.source_line
        {
            return Err("trusted Verify source mapping changed after Apply".into());
        }
        let postimage = trusted_fix::validate_fix_source_policy(&target)?;
        if postimage != record.postimage {
            return Err("trusted Verify source changed after Apply".into());
        }

        let semantic_after =
            trusted_verify::build_semantic_baseline(&session, &snapshot, &record.reference)?;
        let semantic_changes = trusted_verify::compare_semantic_projection(
            &record.semantic_before.selected,
            &semantic_after.selected,
        );
        let regression_signals = trusted_verify::compare_issue_fingerprints(
            &record.semantic_before.console_issues,
            &semantic_after.console_issues,
            &record.semantic_before.network_issues,
            &semantic_after.network_issues,
        );

        let mut visual_diff_evidence_id = None;
        let visual_facts = if let Some(before) = record.visual_before.as_ref() {
            match visual_capture::capture_registered_verification_current(
                app.clone(),
                &visual_state,
                record.session_id,
            )
            .await
            {
                Ok(registered) => {
                    let frame = &registered.frame;
                    let frame_route = visual_capture::canonical_visual_diff_route(&frame.route)?;
                    if frame_route != record.canonical_route {
                        return Err("trusted Verify route changed during verification".into());
                    }
                    let facts = trusted_verify::compare_visual_facts(
                        before,
                        &frame.png,
                        &frame.viewport,
                        semantic_after.selected.rect.as_ref(),
                    )?;
                    if let Some(changed_ratio) = facts.viewport_changed_ratio {
                        visual_diff_evidence_id = Some(
                            visual_capture::register_verification_visual_diff_evidence(
                                record.session_id,
                                frame.route.clone(),
                                frame.viewport.clone(),
                                frame.captured_at_unix_ms,
                                changed_ratio,
                                registered.evidence_id,
                            )
                            .await?,
                        );
                    }
                    facts
                }
                Err(_) => trusted_verify::VisualVerificationFacts {
                    viewport_changed_ratio: None,
                    target_changed_ratio: None,
                },
            }
        } else {
            trusted_verify::VisualVerificationFacts {
                viewport_changed_ratio: None,
                target_changed_ratio: None,
            }
        };

        let comparison = trusted_verify::classify_verification_status(
            semantic_changes,
            regression_signals,
            &visual_facts,
            record.scope,
            record.semantic_before.selected.interactive,
            semantic_after.selected.interactive,
        );

        let post_route =
            visual_capture::managed_surface_canonical_route(&app, record.session_id)?;
        if post_route != record.canonical_route {
            return Err("trusted Verify route changed during verification".into());
        }

        Ok::<trusted_verify::HumanVerifyChangeReceipt, String>(
            trusted_verify::HumanVerifyChangeReceipt {
                verification_id: record.verification_id.clone(),
                reference: record.reference.clone(),
                display_file: record.display_file.clone(),
                scope: record.scope,
                status: comparison.deterministic_status,
                semantic_changes: comparison.semantic_changes,
                regression_signals: comparison.regression_signals,
                viewport_changed_ratio: comparison.viewport_changed_ratio,
                target_changed_ratio: comparison.target_changed_ratio,
                visual_diff_evidence_id,
                snapshot_version: snapshot.version,
                provider_label: None,
                advisory_summary: None,
                verified_at_unix_ms: trusted_verify::now_unix_ms(),
            },
        )
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err("trusted Verify deadline exceeded".to_string()),
    };

    match result {
        Ok(receipt) => {
            verification_store.complete(&verification_id)?;
            Ok(receipt)
        }
        Err(error) => {
            let lower = error.to_ascii_lowercase();
            if lower.contains("route changed")
                || lower.contains("source changed")
                || lower.contains("source mapping changed")
                || lower.contains("target is unavailable")
                || lower.contains("selection is no longer available")
                || lower.contains("selection is ambiguous")
                || lower.contains("element reference")
            {
                let _ = verification_store.invalidate(&verification_id);
            } else {
                let _ = verification_store.release_retryable(&verification_id);
            }
            Err(error)
        }
    }
}

#[tauri::command]
async fn open_source_for_selection(
    app: tauri::AppHandle,
    session_id: SessionId,
    reference: String,
) -> Result<HumanSourceOpenReceipt, String> {
    validate_source_reference(&reference)?;
    let pre_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    let token = read_token().await?;
    let client = control_client()?;

    let session = client
        .get(format!("http://127.0.0.1:45454/v1/sessions/{session_id}"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| "trusted source runtime unavailable".to_string())?
        .error_for_status()
        .map_err(|_| "trusted source session is unavailable".to_string())?
        .json::<Session>()
        .await
        .map_err(|_| "trusted source session is unavailable".to_string())?;

    let snapshot = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/semantic-snapshot/fresh"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| "trusted source runtime unavailable".to_string())?
        .error_for_status()
        .map_err(|_| "trusted source mapping is unavailable".to_string())?
        .json::<PageSnapshot>()
        .await
        .map_err(|_| "trusted source mapping is unavailable".to_string())?;

    let snapshot_route = visual_capture::canonical_visual_diff_route(&snapshot.route)?;
    if snapshot_route != pre_route {
        return Err("trusted source route changed before resolution".into());
    }

    let source = resolve_snapshot_source(&snapshot, &reference)?;
    let project_root = session
        .project
        .git_root
        .as_deref()
        .or(session.project.cwd.as_deref())
        .ok_or_else(|| "trusted source project root is unavailable".to_string())?;

    let target = resolve_trusted_source_target(
        session_id,
        &reference,
        project_root,
        source,
        snapshot.version,
        &pre_route,
    )?;

    let post_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    if pre_route != post_route {
        return Err("trusted source route changed while resolution was in flight".into());
    }

    let launcher = launch_trusted_source(&target)?;
    Ok(HumanSourceOpenReceipt {
        reference,
        display_file: target.project_relative_file,
        line: target.line,
        column: target.column,
        launcher,
        snapshot_version: target.snapshot_version,
    })
}

#[tauri::command]
async fn measure_current_selection(
    app: tauri::AppHandle,
    session_id: SessionId,
    reference: String,
) -> Result<ElementMeasureReceipt, String> {
    validate_measure_reference(&reference)?;
    let pre_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;

    let token = read_token().await?;
    let client = control_client()?;
    let action = client
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions"
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "reference": reference,
            "action": {"type": "measure"}
        }))
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<BridgeAction>()
        .await
        .map_err(err)?;

    if action.session_id != session_id
        || action.reference.as_deref() != Some(reference.as_str())
        || !matches!(action.action, BridgeActionKind::Measure)
    {
        return Err("trusted Measure queue acknowledgement mismatch".into());
    }

    let result = tokio::time::timeout(MEASURE_RESULT_TIMEOUT, async {
        loop {
            let results = client
                .get(format!(
                    "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/results"
                ))
                .bearer_auth(&token)
                .send()
                .await
                .map_err(err)?
                .error_for_status()
                .map_err(err)?
                .json::<Vec<BridgeActionResult>>()
                .await
                .map_err(err)?;

            if let Some(result) = results.into_iter().find(|result| result.action_id == action.id) {
                return Ok::<BridgeActionResult, String>(result);
            }
            tokio::time::sleep(MEASURE_RESULT_POLL).await;
        }
    })
    .await
    .map_err(|_| "trusted Measure timed out waiting for the managed preview".to_string())??;

    if !result.ok {
        return Err("trusted Measure could not resolve the selected element".into());
    }

    let post_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    if pre_route != post_route {
        return Err("trusted Measure route changed while measurement was in flight".into());
    }

    validate_measure_payload(
        &reference,
        &pre_route,
        result.payload,
        result.completed_at.timestamp_millis().max(0) as u64,
    )
}

fn validate_measure_reference(reference: &str) -> Result<(), String> {
    if reference.len() > MAX_MEASURE_REFERENCE_BYTES {
        return Err("trusted Measure element reference exceeds the safety bound".into());
    }
    let Some(hash) = reference.strip_prefix("@e") else {
        return Err("trusted Measure requires a LocalView element reference".into());
    };
    if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("trusted Measure element reference is malformed".into());
    }
    Ok(())
}

fn validate_measure_rect(rect: &MeasureRect, field: &str) -> Result<(), String> {
    let right = rect.x + rect.width;
    let bottom = rect.y + rect.height;
    if !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || !right.is_finite()
        || !bottom.is_finite()
        || rect.x.abs() > MAX_MEASURE_ABS_COORDINATE
        || rect.y.abs() > MAX_MEASURE_ABS_COORDINATE
        || rect.width < 0.0
        || rect.height < 0.0
        || rect.width > MAX_MEASURE_CSS_DIMENSION
        || rect.height > MAX_MEASURE_CSS_DIMENSION
    {
        return Err(format!("trusted Measure {field} geometry is outside the safety range"));
    }
    Ok(())
}

fn validate_measure_payload(
    reference: &str,
    expected_route: &str,
    payload: serde_json::Value,
    measured_at_unix_ms: u64,
) -> Result<ElementMeasureReceipt, String> {
    let payload: MeasurePayload = serde_json::from_value(payload)
        .map_err(|_| "trusted Measure returned an invalid geometry payload".to_string())?;

    if payload.reference != reference {
        return Err("trusted Measure result reference mismatch".into());
    }
    validate_measure_rect(&payload.rect, "viewport")?;
    validate_measure_rect(&payload.document_rect, "document")?;

    if (payload.rect.width - payload.document_rect.width).abs() > MEASURE_DIMENSION_TOLERANCE
        || (payload.rect.height - payload.document_rect.height).abs()
            > MEASURE_DIMENSION_TOLERANCE
    {
        return Err("trusted Measure viewport/document dimensions disagree".into());
    }

    if !payload.viewport.width.is_finite()
        || !payload.viewport.height.is_finite()
        || payload.viewport.width <= 0.0
        || payload.viewport.height <= 0.0
        || payload.viewport.width > MAX_MEASURE_CSS_DIMENSION
        || payload.viewport.height > MAX_MEASURE_CSS_DIMENSION
    {
        return Err("trusted Measure viewport is outside the safety range".into());
    }

    let route_url = url::Url::parse(&payload.route)
        .map_err(|_| "trusted Measure returned an invalid route".to_string())?;
    if !workspace_surface::workspace_navigation_allowed(&route_url) {
        return Err("trusted Measure returned a non-loopback route".into());
    }
    let route = visual_capture::canonical_visual_diff_route(&payload.route)?;
    if route != expected_route {
        return Err("trusted Measure result route does not match the managed surface".into());
    }

    Ok(ElementMeasureReceipt {
        reference: payload.reference,
        rect: payload.rect,
        document_rect: payload.document_rect,
        viewport_css_width: payload.viewport.width,
        viewport_css_height: payload.viewport.height,
        route,
        measured_at_unix_ms,
    })
}

#[cfg(test)]
mod trusted_source_validation_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn semantic_node(reference: &str, source: Option<SourceLocation>, children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            reference: reference.to_owned(),
            role: None,
            name: None,
            tag: "div".into(),
            rect: None,
            interactive: true,
            attributes: BTreeMap::new(),
            source,
            children,
        }
    }

    fn snapshot(root: SemanticNode) -> PageSnapshot {
        PageSnapshot {
            version: 7,
            route: "http://127.0.0.1:5173/".into(),
            viewport: (1440, 900),
            root,
            console_errors: Vec::new(),
            failed_requests: Vec::new(),
            captured_at: chrono::Utc::now(),
        }
    }

    fn source(file: &str) -> SourceLocation {
        SourceLocation {
            file: file.into(),
            line: 1,
            column: Some(3),
            component: Some(format!("{file}:42")),
        }
    }

    fn temp_fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "localview-source-v23-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).expect("create source fixture root");
        root
    }

    #[test]
    fn trusted_source_reference_validator_is_fail_closed() {
        for valid in ["@e1", "@e1a2b3", "@eABCDEF"] {
            assert!(validate_source_reference(valid).is_ok(), "{valid}");
        }
        for invalid in ["", "@e", "@e-not-hex", "button#save", "@g123", "../src/App.tsx"] {
            assert!(validate_source_reference(invalid).is_err(), "{invalid}");
        }
        let oversize = format!("@e{}", "a".repeat(MAX_SOURCE_REFERENCE_BYTES));
        assert!(oversize.len() > MAX_SOURCE_REFERENCE_BYTES);
        assert!(validate_source_reference(&oversize).is_err());
    }

    #[test]
    fn trusted_source_relative_path_validator_rejects_escape_and_uri_inputs() {
        for valid in [
            "src/App.tsx",
            "./src/components/Button.tsx",
            "src//components///Button.tsx",
            "Button.tsx",
        ] {
            assert!(validate_relative_source_path(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "",
            "../secret.txt",
            "src/../../secret.txt",
            "/etc/passwd",
            "C:\\Windows\\System32\\drivers\\etc\\hosts",
            "\\\\server\\share\\file.tsx",
            "file://src/App.tsx",
            "https://example.com/App.tsx",
            "src/App.tsx:42",
            "src/\0App.tsx",
        ] {
            assert!(validate_relative_source_path(invalid).is_err(), "{invalid}");
        }
        let oversized = format!("src/{}", "a".repeat(MAX_SOURCE_FILE_BYTES));
        assert!(oversized.len() > MAX_SOURCE_FILE_BYTES);
        assert!(validate_relative_source_path(&oversized).is_err());
    }

    #[test]
    fn trusted_source_snapshot_resolution_requires_one_exact_reference_with_source() {
        let selected = semantic_node("@e1", Some(source("src/Button.tsx")), Vec::new());
        let root = semantic_node("@eroot", None, vec![selected]);
        let snap = snapshot(root);
        let resolved = resolve_snapshot_source(&snap, "@e1").expect("source must resolve");
        assert_eq!(resolved.file, "src/Button.tsx");
        assert!(resolve_snapshot_source(&snap, "@e2").is_err());

        let missing_source = snapshot(semantic_node(
            "@eroot",
            None,
            vec![semantic_node("@e1", None, Vec::new())],
        ));
        assert!(resolve_snapshot_source(&missing_source, "@e1").is_err());

        let duplicate = snapshot(semantic_node(
            "@eroot",
            None,
            vec![
                semantic_node("@e1", Some(source("src/A.tsx")), Vec::new()),
                semantic_node("@e1", Some(source("src/B.tsx")), Vec::new()),
            ],
        ));
        assert!(resolve_snapshot_source(&duplicate, "@e1").is_err());
    }

    #[test]
    fn trusted_source_target_must_be_canonical_regular_file_inside_project_root() {
        let root = temp_fixture("inside");
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(src.join("Button.tsx"), "export const Button = () => null;")
            .expect("write source");

        let receipt = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &source("src/Button.tsx"),
            9,
            "http://127.0.0.1:5173/",
        )
        .expect("trusted target");
        assert_eq!(receipt.project_relative_file, "src/Button.tsx");
        assert!(receipt.canonical_file.starts_with(&receipt.project_root));
        assert_eq!(receipt.line, 1);
        assert_eq!(receipt.column, Some(3));

        let directory_source = SourceLocation {
            file: "src".into(),
            line: 1,
            column: None,
            component: None,
        };
        assert!(resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &directory_source,
            9,
            "http://127.0.0.1:5173/",
        )
        .is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_source_target_rejects_missing_file_and_invalid_line_column() {
        let root = temp_fixture("bounds");
        let bad_line = SourceLocation {
            file: "missing.tsx".into(),
            line: 0,
            column: None,
            component: None,
        };
        assert!(resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &bad_line,
            1,
            "http://127.0.0.1:5173/",
        )
        .is_err());

        let missing = SourceLocation {
            file: "missing.tsx".into(),
            line: 1,
            column: Some(1),
            component: None,
        };
        assert!(resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &missing,
            1,
            "http://127.0.0.1:5173/",
        )
        .is_err());

        std::fs::write(root.join("short.tsx"), "export const onlyLine = true;")
            .expect("write short source");
        let stale_line = SourceLocation {
            file: "short.tsx".into(),
            line: 2,
            column: Some(1),
            component: None,
        };
        let stale_line_error = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &stale_line,
            1,
            "http://127.0.0.1:5173/",
        )
        .expect_err("stale source line must fail closed");
        assert_eq!(stale_line_error, "trusted source line is unavailable");

        let bad_column = SourceLocation {
            file: "short.tsx".into(),
            line: 1,
            column: Some(MAX_SOURCE_COLUMN + 1),
            component: None,
        };
        assert!(resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &bad_column,
            1,
            "http://127.0.0.1:5173/",
        )
        .is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_source_launcher_plan_uses_fixed_program_and_exact_canonical_target() {
        let root = temp_fixture("launcher-plan");
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(src.join("Button.tsx"), "export const Button = () => null;")
            .expect("write source");
        let target = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &source("src/Button.tsx"),
            9,
            "http://127.0.0.1:5173/",
        )
        .expect("trusted target");

        let plan = trusted_source_launch_plan(&target).expect("launch plan");
        assert!(
            !matches!(
                plan.program.to_ascii_lowercase().as_str(),
                "sh" | "bash" | "cmd" | "cmd.exe" | "powershell" | "powershell.exe" | "pwsh"
            ),
            "trusted source launcher must never route through a shell"
        );
        assert!(
            plan.args.iter().any(|arg| arg.as_os_str() == target.canonical_file.as_os_str()),
            "canonical trusted target must be forwarded as one argv item"
        );

        #[cfg(target_os = "linux")]
        {
            assert_eq!(plan.launcher, SourceOpenLauncher::LinuxXdgOpen);
            assert_eq!(plan.program, "xdg-open");
            assert_eq!(plan.args, vec![target.canonical_file.clone().into_os_string()]);
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(plan.launcher, SourceOpenLauncher::MacOpen);
            assert_eq!(plan.program, "/usr/bin/open");
            assert_eq!(plan.args, vec![target.canonical_file.clone().into_os_string()]);
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(plan.launcher, SourceOpenLauncher::WindowsFileProtocolHandler);
            assert_eq!(plan.program, "rundll32.exe");
            assert_eq!(plan.args[0], OsString::from("url.dll,FileProtocolHandler"));
            assert_eq!(plan.args[1], target.canonical_file.clone().into_os_string());
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_source_launcher_treats_shell_metacharacters_as_plain_argv_data() {
        let root = temp_fixture("launcher-metacharacters");
        let filename = "Button; echo not-a-shell.tsx";
        std::fs::write(root.join(filename), "export default null;").expect("write source");
        let target = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &source(filename),
            4,
            "http://127.0.0.1:5173/",
        )
        .expect("trusted target");

        let plan = trusted_source_launch_plan(&target).expect("launch plan");
        assert!(
            plan.args
                .iter()
                .any(|arg| arg.as_os_str() == target.canonical_file.as_os_str())
        );
        assert!(
            !matches!(
                plan.program.to_ascii_lowercase().as_str(),
                "sh" | "bash" | "cmd" | "cmd.exe" | "powershell" | "powershell.exe" | "pwsh"
            )
        );
        assert_eq!(
            plan.args
                .iter()
                .filter(|arg| arg.as_os_str() == target.canonical_file.as_os_str())
                .count(),
            1,
            "trusted file path must remain one argv item"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_source_launcher_executor_is_injectable_and_sanitizes_failure() {
        use std::cell::RefCell;

        let root = temp_fixture("launcher-executor");
        std::fs::write(root.join("App.tsx"), "export default null;").expect("write source");
        let target = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &source("App.tsx"),
            3,
            "http://127.0.0.1:5173/",
        )
        .expect("trusted target");

        let observed = RefCell::new(None::<(String, Vec<OsString>)>);
        let launcher = launch_trusted_source_with(&target, |plan| {
            *observed.borrow_mut() = Some((plan.program.to_owned(), plan.args.clone()));
            Ok(())
        })
        .expect("fake launcher success");
        let observed = observed.into_inner().expect("fake launcher observed plan");
        assert!(!observed.0.is_empty());
        assert!(observed.1.iter().any(|arg| arg.as_os_str() == target.canonical_file.as_os_str()));

        let error = launch_trusted_source_with(&target, |_plan| Err(()))
            .expect_err("fake launcher failure must fail closed");
        assert_eq!(error, "trusted source launcher unavailable");
        assert!(
            !error.contains(target.canonical_file.to_string_lossy().as_ref()),
            "launcher error must not leak an absolute source path"
        );

        let expected = trusted_source_launch_plan(&target).expect("expected plan").launcher;
        assert_eq!(launcher, expected);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn trusted_source_target_allows_symlink_that_resolves_inside_project() {
        use std::os::unix::fs::symlink;

        let root = temp_fixture("symlink-inside");
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create source dir");
        let target_file = src.join("Target.tsx");
        std::fs::write(&target_file, "export const inside = true;").expect("write target");
        symlink(&target_file, root.join("Alias.tsx")).expect("create inside symlink");

        let resolved = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &source("Alias.tsx"),
            2,
            "http://127.0.0.1:5173/",
        )
        .expect("inside-project symlink should resolve");
        assert_eq!(resolved.canonical_file, std::fs::canonicalize(&target_file).unwrap());
        assert_eq!(resolved.project_relative_file, "src/Target.tsx");

        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn trusted_source_target_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let fixture = temp_fixture("symlink");
        let root = fixture.join("project");
        std::fs::create_dir_all(&root).expect("create project");
        let outside = fixture.join("outside.tsx");
        std::fs::write(&outside, "export const secret = true;").expect("write outside");
        symlink(&outside, root.join("escape.tsx")).expect("create symlink");

        let escaped = source("escape.tsx");
        let result = resolve_trusted_source_target(
            uuid::Uuid::new_v4(),
            "@e1",
            root.to_str().expect("utf8 root"),
            &escaped,
            1,
            "http://127.0.0.1:5173/",
        );
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(fixture);
    }
}

#[cfg(test)]
mod measure_validation_tests {
    use super::*;

    fn payload(
        reference: &str,
        route: &str,
        rect: MeasureRect,
        document_rect: MeasureRect,
        viewport_width: f64,
        viewport_height: f64,
    ) -> serde_json::Value {
        serde_json::json!({
            "reference": reference,
            "rect": rect,
            "document_rect": document_rect,
            "viewport": {
                "width": viewport_width,
                "height": viewport_height,
            },
            "route": route,
        })
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> MeasureRect {
        MeasureRect { x, y, width, height }
    }

    #[test]
    fn measure_validation_accepts_stable_reference_and_fractional_geometry() {
        assert!(validate_measure_reference("@e1a2b3").is_ok());
        let receipt = validate_measure_payload(
            "@e1a2b3",
            "http://127.0.0.1:5173/dashboard",
            payload(
                "@e1a2b3",
                "http://127.0.0.1:5173/dashboard?ignored=1",
                rect(-0.1, 12.3, 128.4, 40.0),
                rect(-0.1, 212.3, 128.4, 40.0),
                1440.0,
                900.0,
            ),
            1234,
        )
        .expect("valid trusted measurement");
        assert_eq!(receipt.reference, "@e1a2b3");
        assert_eq!(receipt.rect.width, 128.4);
        assert_eq!(receipt.measured_at_unix_ms, 1234);
    }

    #[test]
    fn measure_validation_rejects_malformed_and_oversize_references() {
        for invalid in ["", "button#save", "@e", "@e-not-hex", "@g123"] {
            assert!(validate_measure_reference(invalid).is_err(), "{invalid}");
        }
        let oversize = format!("@e{}", "a".repeat(MAX_MEASURE_REFERENCE_BYTES));
        assert!(oversize.len() > MAX_MEASURE_REFERENCE_BYTES);
        assert!(validate_measure_reference(&oversize).is_err());
    }

    #[test]
    fn measure_validation_rejects_non_finite_negative_and_absurd_geometry() {
        for invalid in [
            rect(f64::NAN, 0.0, 1.0, 1.0),
            rect(0.0, f64::INFINITY, 1.0, 1.0),
            rect(0.0, 0.0, -0.1, 1.0),
            rect(0.0, 0.0, 1.0, -0.1),
            rect(MAX_MEASURE_ABS_COORDINATE + 1.0, 0.0, 1.0, 1.0),
            rect(0.0, 0.0, MAX_MEASURE_CSS_DIMENSION + 1.0, 1.0),
        ] {
            assert!(validate_measure_rect(&invalid, "test").is_err());
        }
    }

    #[test]
    fn measure_validation_rejects_viewport_and_document_dimension_mismatch() {
        let mismatch = payload(
            "@e1",
            "http://127.0.0.1:5173/",
            rect(0.0, 0.0, 100.0, 40.0),
            rect(0.0, 200.0, 101.0, 40.0),
            1440.0,
            900.0,
        );
        assert!(validate_measure_payload(
            "@e1",
            "http://127.0.0.1:5173/",
            mismatch,
            1,
        )
        .is_err());

        for (width, height) in [
            (0.0, 900.0),
            (1440.0, -1.0),
            (MAX_MEASURE_CSS_DIMENSION + 1.0, 900.0),
        ] {
            let viewport = payload(
                "@e1",
                "http://127.0.0.1:5173/",
                rect(0.0, 0.0, 100.0, 40.0),
                rect(0.0, 200.0, 100.0, 40.0),
                width,
                height,
            );
            assert!(validate_measure_payload(
                "@e1",
                "http://127.0.0.1:5173/",
                viewport,
                1,
            )
            .is_err());
        }

        let malformed_viewport = serde_json::json!({
            "reference": "@e1",
            "rect": {"x": 0.0, "y": 0.0, "width": 100.0, "height": 40.0},
            "document_rect": {"x": 0.0, "y": 200.0, "width": 100.0, "height": 40.0},
            "viewport": {"width": "NaN", "height": 900.0},
            "route": "http://127.0.0.1:5173/"
        });
        assert!(validate_measure_payload(
            "@e1",
            "http://127.0.0.1:5173/",
            malformed_viewport,
            1,
        )
        .is_err());
    }

    #[test]
    fn measure_validation_rejects_reference_route_and_origin_mismatch() {
        let base = || {
            payload(
                "@e1",
                "http://127.0.0.1:5173/dashboard",
                rect(0.0, 0.0, 100.0, 40.0),
                rect(0.0, 200.0, 100.0, 40.0),
                1440.0,
                900.0,
            )
        };
        assert!(validate_measure_payload(
            "@e2",
            "http://127.0.0.1:5173/dashboard",
            base(),
            1,
        )
        .is_err());
        assert!(validate_measure_payload(
            "@e1",
            "http://127.0.0.1:5173/other",
            base(),
            1,
        )
        .is_err());

        let external = payload(
            "@e1",
            "https://example.com/dashboard",
            rect(0.0, 0.0, 100.0, 40.0),
            rect(0.0, 200.0, 100.0, 40.0),
            1440.0,
            900.0,
        );
        assert!(validate_measure_payload(
            "@e1",
            "http://127.0.0.1:5173/dashboard",
            external,
            1,
        )
        .is_err());
    }

    #[test]
    fn measure_request_lifecycle_is_strictly_bounded() {
        assert!(MEASURE_RESULT_TIMEOUT <= std::time::Duration::from_millis(2_500));
        assert!(MEASURE_RESULT_POLL >= std::time::Duration::from_millis(40));
        assert!(MEASURE_RESULT_POLL <= std::time::Duration::from_millis(75));
    }
}

#[tauri::command]
async fn pause_runtime() -> Result<(), String> {
    post_control("/v1/runtime/pause").await
}

#[tauri::command]
async fn resume_runtime() -> Result<(), String> {
    post_control("/v1/runtime/resume").await
}

#[tauri::command]
async fn open_preview(
    app: tauri::AppHandle,
    registry: tauri::State<'_, workspace_surface::surface_registry::DesktopSurfaceRegistry>,
    session_id: String,
    url: String,
    title: String,
) -> Result<(), String> {
    use workspace_surface::surface_registry::{DesktopSurfaceKind, DesktopSurfaceVisibility};

    let session = session_id.parse::<SessionId>().map_err(err)?;
    let label = preview_label(session);
    if let Some(window) = app.get_webview_window(&label) {
        let current = registry.current(
            session,
            DesktopSurfaceKind::PreviewWindow,
            &label,
        )
        .ok_or_else(|| "preview platform window exists without desktop owner truth".to_string())?;
        window.show().map_err(err)?;
        registry
            .set_visibility(&current.identity, DesktopSurfaceVisibility::Visible)
            .map_err(preview_registry_error)?;
        if let Err(error) = workspace_surface::surface_resource::update_surface_visibility(
            &current.identity,
            DesktopSurfaceVisibility::Visible,
        )
        .await
        {
            let _ = registry.set_visibility(&current.identity, current.visibility);
            if current.visibility == DesktopSurfaceVisibility::Hidden {
                let _ = window.hide();
            }
            return Err(error);
        }
        window.set_focus().map_err(err)?;
        return Ok(());
    }

    let parsed = url::Url::parse(&url).map_err(err)?;
    if !preview_navigation_allowed(&parsed) {
        return Err("LocalView preview refuses non-loopback top-level navigation".into());
    }

    let identity = registry.next_identity(
        session,
        DesktopSurfaceKind::PreviewWindow,
        label.clone(),
    );
    let reservation = workspace_surface::surface_resource::reserve_surface(session).await?;
    let initialization_script = format!(
        "{}\n{}",
        bootstrap_script(&InstrumentationConfig::default()),
        preview_bridge_script(session)
    );

    let window = match WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(format!("{title} — LocalView"))
        .inner_size(1280.0, 820.0)
        .min_inner_size(640.0, 480.0)
        .initialization_script(initialization_script)
        .on_navigation(preview_navigation_allowed)
        .build()
    {
        Ok(window) => window,
        Err(error) => {
            let create_error = err(error);
            if let Err(cancel_error) =
                workspace_surface::surface_resource::cancel_surface_reservation(&reservation).await
            {
                return Err(format!(
                    "{create_error}; failed to cancel preview surface reservation: {cancel_error}"
                ));
            }
            return Err(create_error);
        }
    };

    if let Err(error) = registry.record_created(
        identity.clone(),
        DesktopSurfaceVisibility::Visible,
    ) {
        if let Err(close_error) = window.close() {
            return Err(format!(
                "{}; failed to close preview window after owner-record failure: {close_error}",
                preview_registry_error(error)
            ));
        }
        let _ = workspace_surface::surface_resource::cancel_surface_reservation(&reservation).await;
        return Err(preview_registry_error(error));
    }

    let activation_result = workspace_surface::surface_resource::activate_surface(
        &reservation,
        &identity,
        DesktopSurfaceVisibility::Visible,
    )
    .await;
    if let Err(error) = activation_result {
        if let Err(close_error) = window.close() {
            return Err(format!(
                "{error}; failed to close preview window after activation failure: {close_error}"
            ));
        }
        let _ = registry.record_closed(&identity);
        let _ = workspace_surface::surface_resource::cancel_surface_reservation(&reservation).await;
        let _ = workspace_surface::surface_resource::release_surface(&identity).await;
        return Err(error);
    }

    install_preview_surface_destroyed_reconciler(app, &window, identity);
    Ok(())
}

fn install_preview_surface_destroyed_reconciler(
    app: tauri::AppHandle,
    window: &tauri::WebviewWindow,
    identity: workspace_surface::surface_registry::DesktopSurfaceIdentity,
) {
    window.on_window_event(move |event| {
        if !matches!(event, tauri::WindowEvent::Destroyed) {
            return;
        }
        let registry = app
            .state::<workspace_surface::surface_registry::DesktopSurfaceRegistry>();
        if registry.record_closed(&identity).is_err() {
            return;
        }
        let identity = identity.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = workspace_surface::surface_resource::release_surface(&identity).await {
                eprintln!("LocalView preview surface release failed: {error}");
            }
        });
    });
}

fn preview_registry_error(
    error: workspace_surface::surface_registry::DesktopSurfaceRegistryError,
) -> String {
    format!("desktop preview surface owner registry rejected lifecycle transition: {error:?}")
}

#[tauri::command]
async fn preview_ingest(
    webview_window: tauri::WebviewWindow,
    batch: ObserverBatch,
) -> Result<IngestReport, String> {
    ensure_preview_caller(&webview_window, batch.session_id)?;
    let token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{}/observer",
            batch.session_id
        ))
        .bearer_auth(token)
        .json(&batch)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<IngestReport>()
        .await
        .map_err(err)
}

#[tauri::command]
async fn preview_take_actions(
    webview_window: tauri::WebviewWindow,
    session_id: SessionId,
) -> Result<Vec<serde_json::Value>, String> {
    ensure_preview_caller(&webview_window, session_id)?;
    let token = read_token().await?;
    let client = control_client()?;

    let internal_actions = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/capture-actions"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<PrivateBridgeAction>>()
        .await
        .map_err(err)?;
    let public_actions = client
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<BridgeAction>>()
        .await
        .map_err(err)?;

    let mut actions = Vec::with_capacity(internal_actions.len() + public_actions.len());
    for action in internal_actions {
        actions.push(serde_json::to_value(action).map_err(err)?);
    }
    for action in public_actions {
        actions.push(serde_json::to_value(action).map_err(err)?);
    }
    Ok(actions)
}

#[tauri::command]
async fn preview_take_network_fault_controls(
    webview_window: tauri::WebviewWindow,
    registry: tauri::State<'_, workspace_surface::surface_registry::DesktopSurfaceRegistry>,
    session_id: SessionId,
) -> Result<Vec<NetworkFaultControlRequest>, String> {
    network_fault_preview_surface(registry.inner(), &webview_window, session_id)?;
    let token = read_token().await?;
    control_client()?
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/network-fault-controls"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json::<Vec<NetworkFaultControlRequest>>()
        .await
        .map_err(err)
}

#[tauri::command]
async fn preview_complete_network_fault_control(
    webview_window: tauri::WebviewWindow,
    registry: tauri::State<'_, workspace_surface::surface_registry::DesktopSurfaceRegistry>,
    session_id: SessionId,
    mut result: NetworkFaultControlResult,
) -> Result<(), String> {
    let surface = network_fault_preview_surface(registry.inner(), &webview_window, session_id)?;
    if let Some(payload) = result.payload.as_object_mut() {
        payload.insert(
            "surface_incarnation".into(),
            serde_json::Value::from(surface.identity.incarnation),
        );
    }
    let token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/network-fault-controls/results"
        ))
        .bearer_auth(token)
        .json(&result)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn preview_action_cancellation(
    webview_window: tauri::WebviewWindow,
    session_id: SessionId,
    action_id: uuid::Uuid,
) -> Result<bool, String> {
    ensure_preview_caller(&webview_window, session_id)?;
    let token = read_token().await?;
    let response = control_client()?
        .get(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/cancellations/{action_id}"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(false);
    }
    let signal = response
        .error_for_status()
        .map_err(err)?
        .json::<ActionCancellationSignal>()
        .await
        .map_err(err)?;
    if signal.action_id != action_id {
        return Err("action cancellation signal/action mismatch".into());
    }
    Ok(true)
}

#[tauri::command]
async fn preview_ack_action_cancellation(
    webview_window: tauri::WebviewWindow,
    session_id: SessionId,
    action_id: uuid::Uuid,
) -> Result<(), String> {
    ensure_preview_caller(&webview_window, session_id)?;
    let token = read_token().await?;
    control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/cancellations/{action_id}/ack"
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn preview_complete_action(
    webview_window: tauri::WebviewWindow,
    session_id: SessionId,
    result: BridgeActionResult,
) -> Result<(), String> {
    ensure_preview_caller(&webview_window, session_id)?;
    let token = read_token().await?;
    let response = control_client()?
        .post(format!(
            "http://127.0.0.1:45454/v1/sessions/{session_id}/actions/results"
        ))
        .bearer_auth(token)
        .json(&result)
        .send()
        .await
        .map_err(err)?;
    if response.status() == reqwest::StatusCode::CONFLICT {
        return Ok(());
    }
    response.error_for_status().map_err(err)?;
    Ok(())
}

fn network_fault_preview_surface(
    registry: &workspace_surface::surface_registry::DesktopSurfaceRegistry,
    webview_window: &tauri::WebviewWindow,
    session_id: SessionId,
) -> Result<workspace_surface::surface_registry::DesktopSurfaceSnapshot, String> {
    use workspace_surface::surface_registry::DesktopSurfaceKind;

    let label = workspace_surface::preview_surface_label(session_id);
    if webview_window.label() != label {
        return Err("network fault control requires the exact LocalView preview surface".into());
    }

    let current = registry
        .current(session_id, DesktopSurfaceKind::PreviewWindow, &label)
        .ok_or_else(|| "network fault preview surface is not live in desktop owner registry".to_string())?;
    if current.identity.owner_instance_id != registry.owner_instance_id() {
        return Err("network fault preview surface owner mismatch".into());
    }
    Ok(current)
}

fn ensure_preview_caller(
    webview_window: &tauri::WebviewWindow,
    session_id: SessionId,
) -> Result<(), String> {
    if !workspace_surface::bridge_surface_label_allowed(webview_window.label(), session_id) {
        return Err("preview bridge session/window mismatch".into());
    }
    Ok(())
}

fn preview_label(session_id: SessionId) -> String {
    workspace_surface::preview_surface_label(session_id)
}

fn preview_navigation_allowed(url: &url::Url) -> bool {
    workspace_surface::workspace_navigation_allowed(url)
}

async fn post_control(path: &str) -> Result<(), String> {
    let token = read_token().await?;
    control_client()?
        .post(format!("http://127.0.0.1:45454{path}"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    Ok(())
}

fn control_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(err)
}

async fn read_token() -> Result<String, String> {
    tokio::fs::read_to_string(state_dir()?.join("control.token"))
        .await
        .map(|value| value.trim().to_owned())
        .map_err(err)
}

fn state_dir() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .map(|path| path.join("LocalView"))
        .ok_or_else(|| "no local data directory".into())
}

fn err<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn native_engine() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "WebView2 via Tauri/WRY"
    }
    #[cfg(target_os = "macos")]
    {
        "WKWebView via Tauri/WRY"
    }
    #[cfg(target_os = "linux")]
    {
        "WebKitGTK via Tauri/WRY"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "Tauri/WRY"
    }
}

fn preview_bridge_script(session_id: SessionId) -> String {
    let session = serde_json::to_string(&session_id.to_string())
        .expect("session UUID serializes to JSON string");
    PREVIEW_BRIDGE_SCRIPT.replace("__LOCALVIEW_SESSION_ID__", &session)
}

const PREVIEW_BRIDGE_SCRIPT: &str = r#"
(() => {
  if (window.__LOCALVIEW_NATIVE_BRIDGE__) return;
  const sessionId = __LOCALVIEW_SESSION_ID__;
  const generation = Date.now();
  let running = true;
  let busy = false;
  const pendingActions = new Map();
  const pendingNetworkFaultControls = new Map();

  const MAX_PRIVATE_MASK_SELECTORS = 16;
  const MAX_PRIVATE_MASK_SELECTOR_BYTES = 256;
  const MAX_PRIVATE_MASK_ELEMENTS = 4096;
  const MAX_PRIVATE_MASK_RECTS = 256;
  const MAX_PRIVATE_MASK_VIEWPORT = 100000;

  const eventKind = (type) => ({
    dom_changed: 'dom_mutation',
    geometry_changed: 'layout',
    semantic_snapshot: 'semantic_snapshot',
    route_changed: 'route',
    focus_changed: 'focus',
    scroll_changed: 'scroll',
    console: 'console',
    network: 'network',
    exception: 'runtime_error',
    unhandled_rejection: 'runtime_error',
    long_task: 'performance',
    layout_shift: 'performance',
  })[type] || null;

  const eventTime = (raw) => {
    const offset = Number(raw.at);
    const millis = Number.isFinite(offset) ? performance.timeOrigin + offset : Date.now();
    return new Date(millis).toISOString();
  };

  const normalizeEvents = (events) => events.flatMap((raw) => {
    const kind = eventKind(raw.type);
    if (!kind) return [];
    return [{
      seq: Number(raw.seq) || 0,
      captured_at: eventTime(raw),
      kind,
      reference: raw.ref || raw.refs?.[0] || null,
      route: raw.route || null,
      payload: raw,
    }];
  });

  const resolveRef = (reference) => {
    if (!reference) return null;
    const api = window.__LOCALVIEW__;
    if (!api?.refFor) return null;
    const active = document.activeElement;
    if (active && api.refFor(active) === reference) return active;
    const preferred = document.querySelectorAll('a[href],button,input,select,textarea,summary,[role],[tabindex]');
    for (const element of preferred) {
      if (api.refFor(element) === reference) return element;
    }
    for (const element of document.querySelectorAll('*')) {
      if (api.refFor(element) === reference) return element;
    }
    return null;
  };

  const setElementValue = (element, text, clearFirst) => {
    const next = clearFirst ? text : `${element.value ?? element.textContent ?? ''}${text}`;
    if (element instanceof HTMLInputElement) {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setter?.call(element, next);
    } else if (element instanceof HTMLTextAreaElement) {
      const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      setter?.call(element, next);
    } else if (element.isContentEditable) {
      element.textContent = next;
    } else {
      throw new Error('target does not accept text input');
    }
    element.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    element.dispatchEvent(new Event('change', { bubbles: true, composed: true }));
    return next;
  };

  const keyboardOptions = (action) => {
    const modifiers = new Set((action.modifiers || []).map((value) => String(value).toLowerCase()));
    return {
      key: action.key,
      bubbles: true,
      composed: true,
      cancelable: true,
      altKey: modifiers.has('alt'),
      ctrlKey: modifiers.has('ctrl') || modifiers.has('control'),
      metaKey: modifiers.has('meta') || modifiers.has('cmd') || modifiers.has('command'),
      shiftKey: modifiers.has('shift'),
    };
  };

  const privateMaskGeometry = (rawSelectors) => {
    const viewportWidth = Number(window.innerWidth);
    const viewportHeight = Number(window.innerHeight);
    if (!Number.isFinite(viewportWidth)
      || !Number.isFinite(viewportHeight)
      || viewportWidth <= 0
      || viewportHeight <= 0
      || viewportWidth > MAX_PRIVATE_MASK_VIEWPORT
      || viewportHeight > MAX_PRIVATE_MASK_VIEWPORT) {
      throw new Error('visual_mask_viewport_invalid');
    }

    const selectors = Array.isArray(rawSelectors) ? rawSelectors : [];
    if (selectors.length > MAX_PRIVATE_MASK_SELECTORS) {
      throw new Error('visual_mask_selector_budget_exceeded');
    }

    const seen = new Set();
    const maskRects = [];
    let maskedElements = 0;
    for (const rawSelector of selectors) {
      const selector = String(rawSelector || '');
      if (!selector || new TextEncoder().encode(selector).length > MAX_PRIVATE_MASK_SELECTOR_BYTES) {
        throw new Error('visual_mask_selector_invalid');
      }

      let matches;
      try {
        matches = document.querySelectorAll(selector);
      } catch (_) {
        throw new Error('visual_mask_selector_invalid');
      }

      for (const element of matches) {
        if (seen.has(element)) continue;
        seen.add(element);
        maskedElements += 1;
        if (maskedElements > MAX_PRIVATE_MASK_ELEMENTS) {
          throw new Error('visual_mask_geometry_budget_exceeded');
        }

        for (const rawRect of Array.from(element.getClientRects())) {
          const x = Number(rawRect.x);
          const y = Number(rawRect.y);
          const width = Number(rawRect.width);
          const height = Number(rawRect.height);
          if (![x, y, width, height].every(Number.isFinite)
            || width < 0
            || height < 0
            || !Number.isFinite(x + width)
            || !Number.isFinite(y + height)) {
            throw new Error('visual_mask_geometry_invalid');
          }

          const left = Math.max(0, Math.min(viewportWidth, x));
          const top = Math.max(0, Math.min(viewportHeight, y));
          const right = Math.max(0, Math.min(viewportWidth, x + width));
          const bottom = Math.max(0, Math.min(viewportHeight, y + height));
          if (right <= left || bottom <= top) continue;
          if (maskRects.length >= MAX_PRIVATE_MASK_RECTS) {
            throw new Error('visual_mask_geometry_budget_exceeded');
          }
          maskRects.push({
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
          });
        }
      }
    }

    return {
      viewport_css_width: viewportWidth,
      viewport_css_height: viewportHeight,
      masked_elements: maskedElements,
      mask_rects: maskRects,
    };
  };

  const execute = async (queued) => {
    const action = queued.action || {};
    const target = queued.reference ? resolveRef(queued.reference) : null;
    switch (action.type) {
      case 'click':
        if (!target) throw new Error(`element reference not found: ${queued.reference}`);
        target.click();
        return { reference: queued.reference };
      case 'type_text':
        if (!target) throw new Error(`element reference not found: ${queued.reference}`);
        target.focus?.();
        return { reference: queued.reference, value: setElementValue(target, String(action.text ?? ''), !!action.clear_first) };
      case 'key': {
        const receiver = target || document.activeElement || document.body;
        const options = keyboardOptions(action);
        receiver.dispatchEvent(new KeyboardEvent('keydown', options));
        receiver.dispatchEvent(new KeyboardEvent('keyup', options));
        return { reference: queued.reference || null, key: action.key };
      }
      case 'scroll':
        window.scrollBy({ left: Number(action.x) || 0, top: Number(action.y) || 0, behavior: 'auto' });
        return { x: scrollX, y: scrollY };
      case 'focus':
        if (!target) throw new Error(`element reference not found: ${queued.reference}`);
        target.focus?.({ preventScroll: true });
        return { reference: queued.reference };
      case 'snapshot':
        return window.__LOCALVIEW__?.snapshot?.() ?? null;
      case 'freeze_visuals': {
        const requestedLeaseMs = Number(queued.private_capture?.visual_freeze_lease_ms);
        const leaseMs = Number.isFinite(requestedLeaseMs) ? requestedLeaseMs : 8000;
        const frozen = await window.__LOCALVIEW__?.freezeVisuals?.(queued.id, leaseMs) ?? null;
        if (!frozen) throw new Error('visual_freeze_ack_missing');
        try {
          const geometry = privateMaskGeometry(queued.private_capture?.mask_selectors || []);
          return { ...frozen, ...geometry };
        } catch (error) {
          try { window.__LOCALVIEW__?.restoreVisuals?.(queued.id); } catch (_) {}
          throw error;
        }
      }
      case 'capture_scroll_to': {
        const scrolled = await window.__LOCALVIEW__?.captureScrollTo?.(action.token, action.y) ?? null;
        if (!scrolled) throw new Error('capture_scroll_ack_missing');
        return scrolled;
      }
      case 'capture_tile_probe': {
        const probe = await window.__LOCALVIEW__?.captureTileProbe?.(action.token) ?? null;
        if (!probe) throw new Error('capture_tile_probe_ack_missing');
        const geometry = privateMaskGeometry(queued.private_capture?.mask_selectors || []);
        return { ...probe, ...geometry };
      }
      case 'restore_visuals':
        return window.__LOCALVIEW__?.restoreVisuals?.(String(action.token || '')) ?? null;
      case 'measure': {
        if (!queued.reference) throw new Error('measure requires an element reference');
        const api = window.__LOCALVIEW__;
        const inspected = api?.inspect?.(queued.reference) ?? null;
        if (!inspected) throw new Error('measure element reference unavailable');
        return {
          reference: inspected.reference,
          rect: inspected.node?.rect ?? null,
          document_rect: inspected.node?.documentRect ?? null,
          viewport: inspected.viewport
            ? { width: inspected.viewport.width, height: inspected.viewport.height }
            : null,
          route: inspected.route ?? null,
        };
      }
      case 'inspect': {
        if (!queued.reference) throw new Error('inspect requires an element reference');
        const api = window.__LOCALVIEW__;
        return api?.inspect?.(queued.reference) ?? null;
      }
      default:
        throw new Error(`unsupported LocalView action: ${action.type}`);
    }
  };

  const complete = async (invoke, action, ok, payload, error) => {
    await invoke('preview_complete_action', {
      sessionId,
      result: {
        action_id: action.id,
        ok,
        error: error || null,
        payload: payload ?? null,
        completed_at: new Date().toISOString(),
      },
    });
  };

  const isInternalCaptureAction = (queued) => {
    const action = queued?.action || {};
    return action.type === 'freeze_visuals' || action.type === 'restore_visuals';
  };

  const cancellationRequested = async (invoke, action) => {
    if (isInternalCaptureAction(action)) return false;
    return !!(await invoke('preview_action_cancellation', {
      sessionId,
      actionId: action.id,
    }));
  };

  const acknowledgeCancellation = async (invoke, action) => {
    await invoke('preview_ack_action_cancellation', {
      sessionId,
      actionId: action.id,
    });
  };

  const rememberTakenActions = (actions) => {
    const batch = Array.isArray(actions) ? actions : [];
    for (const action of batch) {
      if (!action?.id || pendingActions.has(action.id)) continue;
      pendingActions.set(action.id, {
        action,
        executed: false,
        cancellationSeen: false,
        ok: true,
        payload: null,
        actionError: null,
      });
    }
  };

  const rememberTakenNetworkFaultControls = (controls) => {
    const batch = Array.isArray(controls) ? controls : [];
    for (const control of batch) {
      if (!control?.id || pendingNetworkFaultControls.has(control.id)) continue;
      pendingNetworkFaultControls.set(control.id, {
        control,
        executed: false,
        ok: true,
        payload: null,
        controlError: null,
      });
    }
  };

  const executeNetworkFaultControl = (control) => {
    const api = window.__LOCALVIEW__;
    if (!api?.installNetworkFaultPlan || !api?.clearNetworkFaultPlan || !api?.networkFaultState) {
      throw new Error('network_fault_runtime_unavailable');
    }
    const command = control?.command || {};
    if (command.type === 'install') {
      const receipt = api.installNetworkFaultPlan(String(command.lease_token || ''), command.plan);
      return { ...receipt, ...api.networkFaultState() };
    }
    if (command.type === 'clear') {
      const receipt = api.clearNetworkFaultPlan(String(command.lease_token || ''));
      return { ...receipt, ...api.networkFaultState() };
    }
    throw new Error('network_fault_control_unsupported');
  };

  const processPendingNetworkFaultControl = async (invoke, entry) => {
    const control = entry.control;
    if (!entry.executed) {
      try {
        entry.payload = executeNetworkFaultControl(control);
        entry.ok = true;
        entry.controlError = null;
      } catch (error) {
        entry.ok = false;
        entry.payload = null;
        entry.controlError = String(error?.message || error);
      } finally {
        entry.executed = true;
      }
    }

    await invoke('preview_complete_network_fault_control', {
      sessionId,
      result: {
        request_id: control.id,
        ok: entry.ok,
        error: entry.controlError,
        payload: entry.payload,
        completed_at: new Date().toISOString(),
      },
    });
    pendingNetworkFaultControls.delete(control.id);
  };

  const processPendingAction = async (invoke, entry) => {
    const cancellationDefaults = { cancellationSeen: false };
    if (entry.cancellationSeen === undefined) {
      Object.assign(entry, cancellationDefaults);
    }
    const action = entry.action;

    if (entry.cancellationSeen) {
      await acknowledgeCancellation(invoke, action);
      pendingActions.delete(action.id);
      return;
    }

    if (!entry.executed) {
      if (await cancellationRequested(invoke, action)) {
        entry.cancellationSeen = true;
        await acknowledgeCancellation(invoke, action);
        pendingActions.delete(action.id);
        return;
      }

      try {
        entry.payload = await execute(action);
        entry.ok = true;
        entry.actionError = null;
      } catch (error) {
        entry.ok = false;
        entry.payload = null;
        entry.actionError = String(error?.message || error);
      } finally {
        entry.executed = true;
      }
    }

    if (await cancellationRequested(invoke, action)) {
      entry.cancellationSeen = true;
      await acknowledgeCancellation(invoke, action);
      pendingActions.delete(action.id);
      return;
    }

    await complete(invoke, action, entry.ok, entry.payload, entry.actionError);
    pendingActions.delete(action.id);
  };

  const tick = async () => {
    if (!running || busy) return;
    const invoke = window.__TAURI__?.core?.invoke;
    if (!invoke) {
      setTimeout(tick, 250);
      return;
    }
    busy = true;
    try {
      const api = window.__LOCALVIEW__;
      const normalized = normalizeEvents(api?.drain?.(256) || []);
      if (normalized.length) {
        await invoke('preview_ingest', {
          batch: { session_id: sessionId, generation, events: normalized },
        });
      }

      if (pendingNetworkFaultControls.size === 0) {
        const controls = await invoke('preview_take_network_fault_controls', { sessionId });
        rememberTakenNetworkFaultControls(controls);
      }

      for (const entry of pendingNetworkFaultControls.values()) {
        try {
          await processPendingNetworkFaultControl(invoke, entry);
        } catch (_) {
          break;
        }
      }

      if (pendingActions.size === 0) {
        const actions = await invoke('preview_take_actions', { sessionId });
        rememberTakenActions(actions);
      }

      for (const entry of pendingActions.values()) {
        try {
          await processPendingAction(invoke, entry);
        } catch (_) {
          break;
        }
      }
    } catch (_) {
      // Best effort by design: LocalView observation must never break the target application.
    } finally {
      busy = false;
      if (running) setTimeout(tick, 140);
    }
  };

  window.__LOCALVIEW_NATIVE_BRIDGE__ = Object.freeze({
    sessionId,
    generation,
    stop() { running = false; },
  });
  setTimeout(tick, 80);
})();
"#;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let _ = app.manage(visual_capture::VisualCaptureState::default());
            let _ = app.manage(trusted_fix::FixProposalStore::default());
            let _ = app.manage(trusted_verify::VerificationStore::default());
            let _ = app.manage(workspace_surface::surface_registry::DesktopSurfaceRegistry::default());
            native_executor_worker::spawn(app.handle().clone());
            let menu = MenuBuilder::new(app)
                .text("show", "Open LocalView")
                .separator()
                .text("quit", "Quit LocalView")
                .build()?;
            TrayIconBuilder::new()
                .tooltip("LocalView — AI-native localhost runtime")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                    if event.id() == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            dashboard_state,
            live_session_state,
            action_correlation,
            ai_provider_capability,
            ask_ai_about_selection,
            ai_fix_capability,
            prepare_fix_proposal,
            apply_fix_proposal,
            discard_fix_proposal,
            verify_fix_change,
            open_source_for_selection,
            measure_current_selection,
            pause_runtime,
            resume_runtime,
            open_preview,
            preview_ingest,
            preview_take_actions,
            preview_take_network_fault_controls,
            preview_complete_network_fault_control,
            preview_action_cancellation,
            preview_ack_action_cancellation,
            preview_complete_action,
            visual_capture::capture_responsive_sweep,
            visual_capture::capture_full_page,
            visual_capture::capture_viewport,
            visual_capture::capture_current_viewport,
            visual_capture::capture_region,
            visual_capture::capture_changed_regions,
            visual_capture::capture_progressive_target,
            visual_capture::capture_visual_packet,
            workspace_surface::workspace_surface_open,
            workspace_surface::workspace_surface_set_bounds,
            workspace_surface::workspace_surface_navigate,
            workspace_surface::workspace_surface_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running LocalView desktop");
}
