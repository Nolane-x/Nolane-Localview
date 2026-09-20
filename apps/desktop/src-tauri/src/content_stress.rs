use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

use localview_protocol::{PageSnapshot, SemanticNode, SessionId};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{visual_capture, workspace_surface};

const MAX_CONTENT_STRESS_SESSIONS: usize = 64;
const MAX_CONTENT_STRESS_TOKEN_BYTES: usize = 128;
const MAX_CONTENT_STRESS_ISSUES: usize = 64;
const CONTENT_STRESS_COMPLETION_TIMEOUT: Duration = Duration::from_secs(3);
const CONTENT_STRESS_POLL: Duration = Duration::from_millis(20);
const RESTORE_GEOMETRY_TOLERANCE_PX: f64 = 1.0;
const NEW_COLLISION_RATIO: f64 = 0.25;
const BASELINE_COLLISION_TOLERANCE: f64 = 0.05;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentStressProfile {
    Expanded130,
    Expanded180,
    DenseCjk,
    RtlPseudo,
}

impl ContentStressProfile {
    const ALL: [Self; 4] = [
        Self::Expanded130,
        Self::Expanded180,
        Self::DenseCjk,
        Self::RtlPseudo,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Expanded130 => "expanded_130",
            Self::Expanded180 => "expanded_180",
            Self::DenseCjk => "dense_cjk",
            Self::RtlPseudo => "rtl_pseudo",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewContentStressCompletion {
    pub request_token: String,
    pub route: String,
    pub status: String,
    pub profile: String,
    pub mutated_nodes: u32,
    pub restored_nodes: u32,
    pub conflict_nodes: u32,
    pub bridge_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StressPhase {
    PendingApply,
    Applied,
    PendingRestore,
    Restored,
    Failed,
}

#[derive(Debug, Clone)]
struct StressEntry {
    token: String,
    route: String,
    profile: ContentStressProfile,
    phase: StressPhase,
    mutated_nodes: u32,
    restored_nodes: u32,
    conflict_nodes: u32,
    bridge_generation: Option<u64>,
    failure: Option<&'static str>,
    started_at: Instant,
}

#[derive(Default)]
pub struct ContentStressState {
    entries: Mutex<HashMap<SessionId, StressEntry>>,
}

impl ContentStressState {
    async fn begin(
        &self,
        session_id: SessionId,
        token: String,
        route: String,
        profile: ContentStressProfile,
    ) -> Result<(), String> {
        let mut entries = self.entries.lock().await;
        if !entries.contains_key(&session_id) && entries.len() >= MAX_CONTENT_STRESS_SESSIONS {
            entries.retain(|_, entry| {
                matches!(
                    entry.phase,
                    StressPhase::PendingApply | StressPhase::Applied | StressPhase::PendingRestore
                )
            });
            if entries.len() >= MAX_CONTENT_STRESS_SESSIONS {
                return Err("content_stress_session_capacity_exceeded".into());
            }
        }
        entries.insert(
            session_id,
            StressEntry {
                token,
                route,
                profile,
                phase: StressPhase::PendingApply,
                mutated_nodes: 0,
                restored_nodes: 0,
                conflict_nodes: 0,
                bridge_generation: None,
                failure: None,
                started_at: Instant::now(),
            },
        );
        Ok(())
    }

    async fn mark_restore_pending(
        &self,
        session_id: SessionId,
        token: &str,
    ) -> Result<(), String> {
        let mut entries = self.entries.lock().await;
        let entry = entries
            .get_mut(&session_id)
            .ok_or_else(|| "content_stress_request_stale".to_string())?;
        if entry.token != token || entry.phase != StressPhase::Applied {
            return Err("content_stress_request_stale".into());
        }
        entry.phase = StressPhase::PendingRestore;
        Ok(())
    }

    async fn complete(
        &self,
        session_id: SessionId,
        caller_route: &str,
        completion: PreviewContentStressCompletion,
    ) -> Result<(), String> {
        validate_token(&completion.request_token)?;
        let route = canonical_route(&completion.route)?;
        if route != caller_route {
            return Err("content_stress_completion_route_mismatch".into());
        }
        let profile = parse_profile(&completion.profile)
            .ok_or_else(|| "content_stress_profile_invalid".to_string())?;

        let mut entries = self.entries.lock().await;
        let Some(entry) = entries.get_mut(&session_id) else {
            return Ok(());
        };
        if entry.token != completion.request_token || entry.profile != profile {
            return Ok(());
        }
        if entry.route != route {
            entry.phase = StressPhase::Failed;
            entry.failure = Some("route_changed");
            return Ok(());
        }
        if completion.bridge_generation == 0 {
            entry.phase = StressPhase::Failed;
            entry.failure = Some("invalid_generation");
            return Ok(());
        }
        match entry.bridge_generation {
            None => entry.bridge_generation = Some(completion.bridge_generation),
            Some(generation) if generation == completion.bridge_generation => {}
            Some(_) => {
                entry.phase = StressPhase::Failed;
                entry.failure = Some("generation_changed");
                return Ok(());
            }
        }

        match completion.status.as_str() {
            "applied" if entry.phase == StressPhase::PendingApply => {
                entry.phase = StressPhase::Applied;
                entry.mutated_nodes = completion.mutated_nodes;
            }
            "restored" if entry.phase == StressPhase::PendingRestore => {
                entry.restored_nodes = completion.restored_nodes;
                entry.conflict_nodes = completion.conflict_nodes;
                if completion.conflict_nodes == 0 {
                    entry.phase = StressPhase::Restored;
                } else {
                    entry.phase = StressPhase::Failed;
                    entry.failure = Some("restore_conflict");
                }
            }
            "restore_conflict" => {
                entry.restored_nodes = completion.restored_nodes;
                entry.conflict_nodes = completion.conflict_nodes;
                entry.phase = StressPhase::Failed;
                entry.failure = Some("restore_conflict");
            }
            "failed" => {
                entry.phase = StressPhase::Failed;
                entry.failure = Some("runtime_failed");
            }
            _ => {}
        }
        Ok(())
    }

    async fn wait_for(
        &self,
        session_id: SessionId,
        token: &str,
        target: StressPhase,
    ) -> Result<StressEntry, String> {
        let deadline = tokio::time::Instant::now() + CONTENT_STRESS_COMPLETION_TIMEOUT;
        loop {
            {
                let entries = self.entries.lock().await;
                let entry = entries
                    .get(&session_id)
                    .ok_or_else(|| "content_stress_request_stale".to_string())?;
                if entry.token != token {
                    return Err("content_stress_request_stale".into());
                }
                if entry.phase == target {
                    return Ok(entry.clone());
                }
                if entry.phase == StressPhase::Failed {
                    return Err(entry
                        .failure
                        .unwrap_or("content_stress_runtime_failed")
                        .to_string());
                }
                if entry.started_at.elapsed() > Duration::from_secs(10) {
                    return Err("content_stress_request_expired".into());
                }
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err("content_stress_completion_timeout".into());
            }
            tokio::time::sleep(CONTENT_STRESS_POLL.min(deadline - now)).await;
        }
    }

    async fn clear(&self, session_id: SessionId, token: &str) {
        let mut entries = self.entries.lock().await;
        if entries
            .get(&session_id)
            .is_some_and(|entry| entry.token == token)
        {
            entries.remove(&session_id);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentStressIssue {
    pub profile: ContentStressProfile,
    pub code: String,
    pub refs: Vec<String>,
    pub confidence: f32,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentStressProfileReceipt {
    pub profile: ContentStressProfile,
    pub synthetic: bool,
    pub mutated_nodes: u32,
    pub snapshot_version: u64,
    pub issue_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentStressReceipt {
    pub route: String,
    pub viewport: (u32, u32),
    pub synthetic: bool,
    pub restored: bool,
    pub profiles: Vec<ContentStressProfileReceipt>,
    pub issues: Vec<ContentStressIssue>,
}

fn parse_profile(value: &str) -> Option<ContentStressProfile> {
    match value {
        "expanded_130" => Some(ContentStressProfile::Expanded130),
        "expanded_180" => Some(ContentStressProfile::Expanded180),
        "dense_cjk" => Some(ContentStressProfile::DenseCjk),
        "rtl_pseudo" => Some(ContentStressProfile::RtlPseudo),
        _ => None,
    }
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty()
        || token.len() > MAX_CONTENT_STRESS_TOKEN_BYTES
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err("content_stress_token_invalid".into());
    }
    Ok(())
}

fn canonical_route(route: &str) -> Result<String, String> {
    visual_capture::canonical_visual_diff_route(route)
        .map_err(|_| "content_stress_route_invalid".to_string())
}

fn managed_eval(
    app: &tauri::AppHandle,
    session_id: SessionId,
    script: &str,
) -> Result<(), String> {
    let preview_label = workspace_surface::preview_surface_label(session_id);
    if let Some(window) = app.get_webview_window(&preview_label) {
        if !workspace_surface::bridge_surface_label_allowed(window.label(), session_id) {
            return Err("content_stress_surface_owner_mismatch".into());
        }
        return window
            .eval(script)
            .map_err(|_| "content_stress_eval_failed".to_string());
    }

    #[cfg(feature = "native-workspace")]
    {
        let workspace_label = workspace_surface::workspace_label(session_id);
        if let Some(webview) = app.get_webview(&workspace_label) {
            if !workspace_surface::bridge_surface_label_allowed(webview.label(), session_id) {
                return Err("content_stress_surface_owner_mismatch".into());
            }
            return webview
                .eval(script)
                .map_err(|_| "content_stress_eval_failed".to_string());
        }
    }

    Err("content_stress_managed_surface_unavailable".into())
}

fn begin_script(token: &str, route: &str, profile: ContentStressProfile) -> Result<String, String> {
    validate_token(token)?;
    let token = serde_json::to_string(token).map_err(|_| "content_stress_token_invalid")?;
    let route = serde_json::to_string(route).map_err(|_| "content_stress_route_invalid")?;
    let profile = serde_json::to_string(profile.as_str())
        .map_err(|_| "content_stress_profile_invalid")?;
    Ok(format!(
        "window.__LOCALVIEW__?.beginContentStress?.({{ requestToken: {token}, route: {route}, profile: {profile} }});"
    ))
}

fn restore_script(token: &str) -> Result<String, String> {
    validate_token(token)?;
    let token = serde_json::to_string(token).map_err(|_| "content_stress_token_invalid")?;
    Ok(format!(
        "window.__LOCALVIEW__?.restoreContentStress?.({token});"
    ))
}

#[derive(Clone)]
struct FlatNode {
    reference: String,
    parent: Option<String>,
    rect: Option<localview_protocol::Rect>,
    interactive: bool,
    name: Option<String>,
    tag: String,
}

fn flatten_snapshot(snapshot: &PageSnapshot) -> BTreeMap<String, FlatNode> {
    fn visit(
        node: &SemanticNode,
        parent: Option<&str>,
        output: &mut BTreeMap<String, FlatNode>,
    ) {
        output.insert(
            node.reference.clone(),
            FlatNode {
                reference: node.reference.clone(),
                parent: parent.map(str::to_owned),
                rect: node.rect.clone(),
                interactive: node.interactive,
                name: node.name.clone(),
                tag: node.tag.clone(),
            },
        );
        for child in &node.children {
            visit(child, Some(&node.reference), output);
        }
    }

    let mut output = BTreeMap::new();
    visit(&snapshot.root, None, &mut output);
    output
}

fn outside_viewport(rect: &localview_protocol::Rect, viewport: (u32, u32)) -> bool {
    rect.x < -1.0
        || rect.y < -1.0
        || rect.x + rect.width > f64::from(viewport.0) + 1.0
        || rect.y + rect.height > f64::from(viewport.1) + 1.0
}

fn overlap_ratio(a: &localview_protocol::Rect, b: &localview_protocol::Rect) -> f64 {
    let x = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let y = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    if x <= 0.0 || y <= 0.0 {
        return 0.0;
    }
    let intersection = x * y;
    intersection / (a.width * a.height).min(b.width * b.height).max(1.0)
}

fn push_issue(issues: &mut Vec<ContentStressIssue>, issue: ContentStressIssue) {
    if issues.len() < MAX_CONTENT_STRESS_ISSUES
        && !issues
            .iter()
            .any(|existing| existing.code == issue.code && existing.refs == issue.refs)
    {
        issues.push(issue);
    }
}

fn analyze_profile(
    profile: ContentStressProfile,
    baseline: &PageSnapshot,
    stressed: &PageSnapshot,
) -> Result<Vec<ContentStressIssue>, String> {
    if canonical_route(&baseline.route)? != canonical_route(&stressed.route)?
        || baseline.viewport != stressed.viewport
    {
        return Err("content_stress_state_drift".into());
    }

    let baseline_nodes = flatten_snapshot(baseline);
    let stressed_nodes = flatten_snapshot(stressed);
    let common = baseline_nodes
        .keys()
        .filter(|reference| stressed_nodes.contains_key(*reference))
        .count();
    let minimum_common = baseline_nodes.len().saturating_mul(4) / 5;
    if common < minimum_common {
        return Err("content_stress_semantic_state_drift".into());
    }

    let mut issues = Vec::new();
    for (reference, before) in &baseline_nodes {
        let Some(after) = stressed_nodes.get(reference) else {
            if before.interactive {
                push_issue(
                    &mut issues,
                    ContentStressIssue {
                        profile,
                        code: "content_interactive_disappeared".into(),
                        refs: vec![reference.clone()],
                        confidence: 0.95,
                        evidence: "interactive_ref_missing_under_synthetic_content_stress".into(),
                    },
                );
            }
            continue;
        };
        if let (Some(before_rect), Some(after_rect)) = (&before.rect, &after.rect)
            && !outside_viewport(before_rect, baseline.viewport)
            && outside_viewport(after_rect, stressed.viewport)
        {
            push_issue(
                &mut issues,
                ContentStressIssue {
                    profile,
                    code: "content_viewport_overflow".into(),
                    refs: vec![reference.clone()],
                    confidence: 1.0,
                    evidence: format!(
                        "before=({:.1},{:.1},{:.1},{:.1}) after=({:.1},{:.1},{:.1},{:.1}) viewport={}x{}",
                        before_rect.x,
                        before_rect.y,
                        before_rect.width,
                        before_rect.height,
                        after_rect.x,
                        after_rect.y,
                        after_rect.width,
                        after_rect.height,
                        stressed.viewport.0,
                        stressed.viewport.1
                    ),
                },
            );
        }
    }

    let mut children_by_parent = BTreeMap::<Option<String>, Vec<String>>::new();
    for node in baseline_nodes.values() {
        children_by_parent
            .entry(node.parent.clone())
            .or_default()
            .push(node.reference.clone());
    }
    for refs in children_by_parent.values() {
        if refs.len() > 32 {
            continue;
        }
        for left_index in 0..refs.len() {
            for right_index in (left_index + 1)..refs.len() {
                let left_ref = &refs[left_index];
                let right_ref = &refs[right_index];
                let (Some(before_left), Some(before_right), Some(after_left), Some(after_right)) = (
                    baseline_nodes.get(left_ref).and_then(|node| node.rect.as_ref()),
                    baseline_nodes.get(right_ref).and_then(|node| node.rect.as_ref()),
                    stressed_nodes.get(left_ref).and_then(|node| node.rect.as_ref()),
                    stressed_nodes.get(right_ref).and_then(|node| node.rect.as_ref()),
                ) else {
                    continue;
                };
                let before_ratio = overlap_ratio(before_left, before_right);
                let after_ratio = overlap_ratio(after_left, after_right);
                if before_ratio <= BASELINE_COLLISION_TOLERANCE && after_ratio >= NEW_COLLISION_RATIO {
                    let mut pair = vec![left_ref.clone(), right_ref.clone()];
                    pair.sort();
                    push_issue(
                        &mut issues,
                        ContentStressIssue {
                            profile,
                            code: "content_sibling_collision".into(),
                            refs: pair,
                            confidence: 0.92,
                            evidence: format!(
                                "overlap_ratio_before={before_ratio:.3} overlap_ratio_after={after_ratio:.3}"
                            ),
                        },
                    );
                }
            }
        }
    }

    Ok(issues)
}

fn validate_restored_snapshot(
    baseline: &PageSnapshot,
    restored: &PageSnapshot,
) -> Result<(), String> {
    if canonical_route(&baseline.route)? != canonical_route(&restored.route)?
        || baseline.viewport != restored.viewport
    {
        return Err("content_stress_restore_validation_failed".into());
    }
    let before = flatten_snapshot(baseline);
    let after = flatten_snapshot(restored);
    if before.len() != after.len() {
        return Err("content_stress_restore_validation_failed".into());
    }
    for (reference, expected) in before {
        let Some(actual) = after.get(&reference) else {
            return Err("content_stress_restore_validation_failed".into());
        };
        if expected.tag != actual.tag
            || expected.name != actual.name
            || expected.interactive != actual.interactive
        {
            return Err("content_stress_restore_validation_failed".into());
        }
        match (&expected.rect, &actual.rect) {
            (Some(left), Some(right)) => {
                if (left.x - right.x).abs() > RESTORE_GEOMETRY_TOLERANCE_PX
                    || (left.y - right.y).abs() > RESTORE_GEOMETRY_TOLERANCE_PX
                    || (left.width - right.width).abs() > RESTORE_GEOMETRY_TOLERANCE_PX
                    || (left.height - right.height).abs() > RESTORE_GEOMETRY_TOLERANCE_PX
                {
                    return Err("content_stress_restore_validation_failed".into());
                }
            }
            (None, None) => {}
            _ => return Err("content_stress_restore_validation_failed".into()),
        }
    }
    Ok(())
}

async fn best_effort_restore(
    app: &tauri::AppHandle,
    state: &ContentStressState,
    session_id: SessionId,
    token: &str,
) {
    let _ = state.mark_restore_pending(session_id, token).await;
    if let Ok(script) = restore_script(token) {
        let _ = managed_eval(app, session_id, &script);
    }
    let _ = state.wait_for(session_id, token, StressPhase::Restored).await;
    state.clear(session_id, token).await;
}

async fn run_profile(
    app: &tauri::AppHandle,
    state: &ContentStressState,
    session_id: SessionId,
    route: &str,
    baseline: &PageSnapshot,
    profile: ContentStressProfile,
) -> Result<(ContentStressProfileReceipt, Vec<ContentStressIssue>), String> {
    let token = format!("stress-{}", Uuid::new_v4().simple());
    state
        .begin(session_id, token.clone(), route.to_owned(), profile)
        .await?;
    if let Err(error) = managed_eval(app, session_id, &begin_script(&token, route, profile)?) {
        best_effort_restore(app, state, session_id, &token).await;
        return Err(error);
    }

    let applied = match state
        .wait_for(session_id, &token, StressPhase::Applied)
        .await
    {
        Ok(entry) => entry,
        Err(error) => {
            best_effort_restore(app, state, session_id, &token).await;
            return Err(error);
        }
    };

    let stressed_result = async {
        visual_capture::wait_for_content_stress_settle(session_id).await?;
        let snapshot = visual_capture::fresh_semantic_snapshot(session_id).await?;
        let issues = analyze_profile(profile, baseline, &snapshot)?;
        Ok::<_, String>((snapshot, issues))
    }
    .await;

    let (stressed, issues) = match stressed_result {
        Ok(value) => value,
        Err(error) => {
            best_effort_restore(app, state, session_id, &token).await;
            return Err(error);
        }
    };

    let restore_result = async {
        state.mark_restore_pending(session_id, &token).await?;
        managed_eval(app, session_id, &restore_script(&token)?)?;
        state
            .wait_for(session_id, &token, StressPhase::Restored)
            .await
    }
    .await;
    let restored_entry = match restore_result {
        Ok(entry) => entry,
        Err(error) => {
            best_effort_restore(app, state, session_id, &token).await;
            return Err(error);
        }
    };
    if restored_entry.conflict_nodes != 0
        || restored_entry.restored_nodes > restored_entry.mutated_nodes
    {
        state.clear(session_id, &token).await;
        return Err("content_stress_restore_conflict".into());
    }
    state.clear(session_id, &token).await;

    visual_capture::wait_for_content_stress_settle(session_id).await?;
    let restored = visual_capture::fresh_semantic_snapshot(session_id).await?;
    validate_restored_snapshot(baseline, &restored)?;

    Ok((
        ContentStressProfileReceipt {
            profile,
            synthetic: true,
            mutated_nodes: applied.mutated_nodes,
            snapshot_version: stressed.version,
            issue_count: issues.len(),
        },
        issues,
    ))
}

#[tauri::command]
pub async fn capture_content_locale_stress(
    app: tauri::AppHandle,
    state: tauri::State<'_, ContentStressState>,
    capture_state: tauri::State<'_, visual_capture::VisualCaptureState>,
    session_id: SessionId,
) -> Result<ContentStressReceipt, String> {
    let capture_gate = visual_capture::session_capture_gate(&capture_state, session_id).await?;
    let _capture_guard = capture_gate.lock().await;
    let managed_route = visual_capture::managed_surface_canonical_route(&app, session_id)?;
    let managed_route = canonical_route(&managed_route)?;
    visual_capture::wait_for_content_stress_settle(session_id).await?;
    let baseline = visual_capture::fresh_semantic_snapshot(session_id).await?;
    let baseline_route = canonical_route(&baseline.route)?;
    if baseline_route != managed_route {
        return Err("content_stress_route_drift".into());
    }

    let mut profiles = Vec::with_capacity(ContentStressProfile::ALL.len());
    let mut issues = Vec::new();
    for profile in ContentStressProfile::ALL {
        let (receipt, mut profile_issues) =
            run_profile(&app, &state, session_id, &managed_route, &baseline, profile).await?;
        profiles.push(receipt);
        for issue in profile_issues.drain(..) {
            if issues.len() >= MAX_CONTENT_STRESS_ISSUES {
                break;
            }
            if !issues.iter().any(|existing: &ContentStressIssue| {
                existing.profile == issue.profile
                    && existing.code == issue.code
                    && existing.refs == issue.refs
            }) {
                issues.push(issue);
            }
        }
    }

    Ok(ContentStressReceipt {
        route: managed_route,
        viewport: baseline.viewport,
        synthetic: true,
        restored: true,
        profiles,
        issues,
    })
}

#[tauri::command]
pub async fn preview_complete_content_stress(
    webview_window: tauri::WebviewWindow,
    state: tauri::State<'_, ContentStressState>,
    session_id: SessionId,
    completion: PreviewContentStressCompletion,
) -> Result<(), String> {
    if !workspace_surface::bridge_surface_label_allowed(webview_window.label(), session_id) {
        return Err("content_stress_bridge_session_window_mismatch".into());
    }
    let caller = webview_window
        .url()
        .map_err(|_| "content_stress_route_unavailable")?;
    if !workspace_surface::workspace_navigation_allowed(&caller) {
        return Err("content_stress_route_not_loopback".into());
    }
    let caller_route = canonical_route(caller.as_str())?;
    state.complete(session_id, &caller_route, completion).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use localview_protocol::{Rect, SemanticNode};
    use std::collections::BTreeMap;

    fn node(reference: &str, rect: Rect, interactive: bool, children: Vec<SemanticNode>) -> SemanticNode {
        SemanticNode {
            reference: reference.into(),
            role: None,
            name: Some(reference.into()),
            tag: "div".into(),
            rect: Some(rect),
            interactive,
            attributes: BTreeMap::new(),
            source: None,
            ownership: None,
            children,
        }
    }

    fn snapshot(root: SemanticNode) -> PageSnapshot {
        PageSnapshot {
            version: 1,
            route: "http://127.0.0.1:5173/".into(),
            viewport: (400, 800),
            root,
            console_errors: vec![],
            failed_requests: vec![],
            captured_at: Utc::now(),
        }
    }

    #[test]
    fn detects_new_viewport_overflow_without_calling_growth_itself_a_failure() {
        let before = snapshot(node(
            "@eroot",
            Rect { x: 0.0, y: 0.0, width: 400.0, height: 800.0 },
            false,
            vec![node(
                "@e1",
                Rect { x: 20.0, y: 20.0, width: 120.0, height: 30.0 },
                true,
                vec![],
            )],
        ));
        let after = snapshot(node(
            "@eroot",
            Rect { x: 0.0, y: 0.0, width: 400.0, height: 800.0 },
            false,
            vec![node(
                "@e1",
                Rect { x: 20.0, y: 20.0, width: 420.0, height: 60.0 },
                true,
                vec![],
            )],
        ));
        let issues = analyze_profile(ContentStressProfile::Expanded180, &before, &after).unwrap();
        assert!(issues.iter().any(|issue| issue.code == "content_viewport_overflow"));
    }

    #[test]
    fn detects_new_sibling_collision() {
        let before = snapshot(node(
            "@eroot",
            Rect { x: 0.0, y: 0.0, width: 400.0, height: 800.0 },
            false,
            vec![
                node("@ea", Rect { x: 10.0, y: 10.0, width: 100.0, height: 40.0 }, true, vec![]),
                node("@eb", Rect { x: 130.0, y: 10.0, width: 100.0, height: 40.0 }, true, vec![]),
            ],
        ));
        let after = snapshot(node(
            "@eroot",
            Rect { x: 0.0, y: 0.0, width: 400.0, height: 800.0 },
            false,
            vec![
                node("@ea", Rect { x: 10.0, y: 10.0, width: 180.0, height: 40.0 }, true, vec![]),
                node("@eb", Rect { x: 130.0, y: 10.0, width: 100.0, height: 40.0 }, true, vec![]),
            ],
        ));
        let issues = analyze_profile(ContentStressProfile::Expanded180, &before, &after).unwrap();
        assert!(issues.iter().any(|issue| issue.code == "content_sibling_collision"));
    }

    #[test]
    fn restore_validation_rejects_semantic_or_geometry_drift() {
        let baseline = snapshot(node(
            "@eroot",
            Rect { x: 0.0, y: 0.0, width: 400.0, height: 800.0 },
            false,
            vec![node("@e1", Rect { x: 10.0, y: 10.0, width: 100.0, height: 40.0 }, true, vec![])],
        ));
        let mut changed = baseline.clone();
        changed.root.children[0].name = Some("different".into());
        assert!(validate_restored_snapshot(&baseline, &changed).is_err());
    }

    #[test]
    fn completion_schema_contains_no_raw_text_fields() {
        let value = serde_json::to_value(ContentStressReceipt {
            route: "http://127.0.0.1:5173/".into(),
            viewport: (400, 800),
            synthetic: true,
            restored: true,
            profiles: vec![],
            issues: vec![],
        })
        .unwrap();
        let encoded = value.to_string();
        for forbidden in ["textContent", "innerHTML", "originalText", "stressedText", "inputValue"] {
            assert!(!encoded.contains(forbidden));
        }
    }
}
