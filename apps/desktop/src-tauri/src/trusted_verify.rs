use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use localview_native_capture::ViewportMeta;
use localview_protocol::{PageSnapshot, Rect, SemanticNode, Session, SessionId};
use localview_visual::{RgbaImage, decode_png_rgba, pixel_diff};
use serde::Serialize;
use uuid::Uuid;

use crate::trusted_ai;

pub const VERIFY_CONTEXT_VERSION: u32 = 1;
pub const MAX_VERIFICATION_RECORDS: usize = 16;
pub const VERIFICATION_TTL: Duration = Duration::from_secs(5 * 60);
pub const MAX_VERIFY_VISUAL_BYTES_PER_RECORD: usize = 16 * 1024 * 1024;
pub const MAX_VERIFY_TOTAL_VISUAL_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_VERIFY_SEMANTIC_BYTES: usize = 48 * 1024;
const VERIFY_PIXEL_THRESHOLD: u8 = 12;
const MAX_VERIFY_CHANGE_CODES: usize = 32;
const MAX_VERIFY_REGRESSION_CODES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationScope {
    SemanticVisual,
    SemanticOnly,
}

impl VerificationScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SemanticVisual => "semantic_visual",
            Self::SemanticOnly => "semantic_only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStatus {
    Pending,
    Verifying,
    Verified,
    Expired,
    Invalidated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeterministicVerificationStatus {
    ChangeObserved,
    NoObservableChange,
    RegressionSignal,
    Inconclusive,
}

impl DeterministicVerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChangeObserved => "change_observed",
            Self::NoObservableChange => "no_observable_change",
            Self::RegressionSignal => "regression_signal",
            Self::Inconclusive => "inconclusive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifySemanticProjection {
    pub reference: String,
    pub role: Option<String>,
    pub name: Option<String>,
    pub tag: String,
    pub interactive: bool,
    pub attributes: BTreeMap<String, String>,
    pub source: Option<String>,
    pub rect: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyConsoleFingerprint {
    pub level: String,
    pub message: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyNetworkFingerprint {
    pub method: String,
    pub path: String,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifySemanticBaseline {
    pub context_version: u32,
    pub snapshot_version: u64,
    pub selected: VerifySemanticProjection,
    pub console_issues: Vec<VerifyConsoleFingerprint>,
    pub network_issues: Vec<VerifyNetworkFingerprint>,
}

#[derive(Debug, Clone)]
pub struct VerifyVisualBaseline {
    pub png: Arc<Vec<u8>>,
    pub viewport: ViewportMeta,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub target_rect: Option<Rect>,
    pub capture_region: Option<Rect>,
    pub captured_at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub struct VerificationRecord {
    pub verification_id: String,
    pub proposal_id: String,
    pub session_id: SessionId,
    pub reference: String,
    pub canonical_route: String,
    pub canonical_file: std::path::PathBuf,
    pub project_root: std::path::PathBuf,
    pub display_file: String,
    pub source_line: u32,
    pub postimage: Vec<u8>,
    pub instruction: String,
    pub semantic_before: VerifySemanticBaseline,
    pub visual_before: Option<VerifyVisualBaseline>,
    pub scope: VerificationScope,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub status: VerificationStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualVerificationFacts {
    pub viewport_changed_ratio: Option<f64>,
    pub target_changed_ratio: Option<f64>,
    pub affected_region_changed_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationComparison {
    pub deterministic_status: DeterministicVerificationStatus,
    pub semantic_changes: Vec<String>,
    pub regression_signals: Vec<String>,
    pub viewport_changed_ratio: Option<f64>,
    pub target_changed_ratio: Option<f64>,
    pub affected_region_changed_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanVerifyChangeReceipt {
    pub verification_id: String,
    pub reference: String,
    pub display_file: String,
    pub scope: VerificationScope,
    pub status: DeterministicVerificationStatus,
    pub semantic_changes: Vec<String>,
    pub regression_signals: Vec<String>,
    pub viewport_changed_ratio: Option<f64>,
    pub target_changed_ratio: Option<f64>,
    pub affected_region_changed_ratio: Option<f64>,
    pub visual_diff_evidence_id: Option<String>,
    pub snapshot_version: u64,
    pub provider_label: Option<String>,
    pub advisory_summary: Option<String>,
    pub verified_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyProviderAdvisory {
    pub provider_label: String,
    pub advisory_summary: String,
}

#[derive(Default)]
pub struct VerificationStore {
    records: Mutex<HashMap<String, VerificationRecord>>,
}

impl VerificationStore {
    fn with_records<T>(
        &self,
        f: impl FnOnce(&mut HashMap<String, VerificationRecord>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| "trusted Verify store unavailable".to_string())?;
        Self::reap_expired_locked(&mut records);
        f(&mut records)
    }

    fn retained_visual_bytes(records: &HashMap<String, VerificationRecord>) -> usize {
        records
            .values()
            .filter_map(|record| record.visual_before.as_ref())
            .map(|visual| visual.png.len())
            .sum()
    }

    fn reap_expired_locked(records: &mut HashMap<String, VerificationRecord>) {
        let now = Instant::now();
        for record in records.values_mut() {
            if matches!(record.status, VerificationStatus::Pending) && record.expires_at <= now {
                record.status = VerificationStatus::Expired;
            }
        }
        records.retain(|_, record| {
            !matches!(
                record.status,
                VerificationStatus::Expired | VerificationStatus::Invalidated
            )
        });
    }

    pub fn reap_expired(&self) -> Result<(), String> {
        self.with_records(|_| Ok(()))
    }

    pub fn insert(&self, record: VerificationRecord) -> Result<(), String> {
        self.with_records(|records| {
            if records.len() >= MAX_VERIFICATION_RECORDS {
                return Err("trusted Verify capacity exceeded".into());
            }
            let visual_bytes = record
                .visual_before
                .as_ref()
                .map(|visual| visual.png.len())
                .unwrap_or(0);
            if visual_bytes > MAX_VERIFY_VISUAL_BYTES_PER_RECORD {
                return Err("trusted Verify visual baseline exceeds per-record bound".into());
            }
            let projected = Self::retained_visual_bytes(records)
                .checked_add(visual_bytes)
                .ok_or_else(|| "trusted Verify visual baseline budget overflow".to_string())?;
            if projected > MAX_VERIFY_TOTAL_VISUAL_BYTES {
                return Err("trusted Verify visual baseline budget exceeded".into());
            }
            records.insert(record.verification_id.clone(), record);
            Ok(())
        })
    }

    pub fn begin_verify(&self, verification_id: &str) -> Result<VerificationRecord, String> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| "trusted Verify store unavailable".to_string())?;
        let now = Instant::now();
        let result = {
            let record = records
                .get_mut(verification_id)
                .ok_or_else(|| "trusted Verify record is unavailable".to_string())?;
            if record.expires_at <= now {
                record.status = VerificationStatus::Expired;
                Err("trusted Verify record expired".to_string())
            } else if record.status != VerificationStatus::Pending {
                Err("trusted Verify record is not pending".to_string())
            } else {
                record.status = VerificationStatus::Verifying;
                Ok(record.clone())
            }
        };
        if result.is_err() {
            Self::reap_expired_locked(&mut records);
        }
        result
    }

    pub fn release_retryable(&self, verification_id: &str) -> Result<(), String> {
        self.with_records(|records| {
            let record = records
                .get_mut(verification_id)
                .ok_or_else(|| "trusted Verify record is unavailable".to_string())?;
            if record.status == VerificationStatus::Verifying {
                record.status = VerificationStatus::Pending;
            }
            Ok(())
        })
    }

    pub fn complete(&self, verification_id: &str) -> Result<(), String> {
        self.with_records(|records| {
            let record = records
                .get_mut(verification_id)
                .ok_or_else(|| "trusted Verify record is unavailable".to_string())?;
            if record.status != VerificationStatus::Verifying {
                return Err("trusted Verify record is not verifying".into());
            }
            record.status = VerificationStatus::Verified;
            records.remove(verification_id);
            Ok(())
        })
    }

    pub fn discard_verification(&self, verification_id: &str) -> Result<(), String> {
        self.with_records(|records| {
            if let Some(record) = records.get_mut(verification_id) {
                record.status = VerificationStatus::Invalidated;
            }
            records.remove(verification_id);
            Ok(())
        })
    }

    pub fn invalidate(&self, verification_id: &str) -> Result<(), String> {
        self.discard_verification(verification_id)
    }

    #[cfg(test)]
    fn retained_visual_bytes_for_test(&self) -> usize {
        self.records
            .lock()
            .map(|records| Self::retained_visual_bytes(&records))
            .unwrap_or(0)
    }
}

fn exact_node<'a>(
    node: &'a SemanticNode,
    reference: &str,
    found: &mut Option<&'a SemanticNode>,
    count: &mut usize,
) {
    if node.reference == reference {
        *count += 1;
        if found.is_none() {
            *found = Some(node);
        }
    }
    for child in &node.children {
        exact_node(child, reference, found, count);
    }
}

fn selected_rect(snapshot: &PageSnapshot, reference: &str) -> Result<Option<Rect>, String> {
    let mut found = None;
    let mut count = 0usize;
    exact_node(&snapshot.root, reference, &mut found, &mut count);
    if count == 0 {
        return Err("trusted Verify target is unavailable".into());
    }
    if count != 1 {
        return Err("trusted Verify target is ambiguous".into());
    }
    Ok(found.and_then(|node| node.rect.clone()))
}

pub fn issue_fingerprint(
    context: &trusted_ai::TrustedAiContext,
) -> (Vec<VerifyConsoleFingerprint>, Vec<VerifyNetworkFingerprint>) {
    let mut console = context
        .console_issues
        .iter()
        .map(|issue| VerifyConsoleFingerprint {
            level: issue.level.clone(),
            message: issue.message.clone(),
            count: issue.count,
        })
        .collect::<Vec<_>>();
    console.sort();

    let mut network = context
        .network_issues
        .iter()
        .map(|issue| VerifyNetworkFingerprint {
            method: issue.method.clone(),
            path: issue.path.clone(),
            status: issue.status,
            error: issue.error.clone(),
        })
        .collect::<Vec<_>>();
    network.sort();
    (console, network)
}

pub fn build_semantic_baseline(
    session: &Session,
    snapshot: &PageSnapshot,
    reference: &str,
) -> Result<VerifySemanticBaseline, String> {
    let context = trusted_ai::build_trusted_ai_context(session, snapshot, reference)?;
    let rect = selected_rect(snapshot, reference)?;
    let (console_issues, network_issues) = issue_fingerprint(&context);

    let baseline = VerifySemanticBaseline {
        context_version: VERIFY_CONTEXT_VERSION,
        snapshot_version: snapshot.version,
        selected: VerifySemanticProjection {
            reference: context.selected.reference,
            role: context.selected.role,
            name: context.selected.name,
            tag: context.selected.tag,
            interactive: context.selected.interactive,
            attributes: context.selected.attributes,
            source: context.selected.source,
            rect,
        },
        console_issues,
        network_issues,
    };
    let bytes = serde_json::to_vec(&baseline)
        .map_err(|_| "trusted Verify semantic baseline serialization failed".to_string())?;
    if bytes.len() > MAX_VERIFY_SEMANTIC_BYTES {
        return Err("trusted Verify semantic baseline exceeds safety bound".into());
    }
    Ok(baseline)
}

pub fn compare_semantic_projection(
    before: &VerifySemanticProjection,
    after: &VerifySemanticProjection,
) -> Vec<String> {
    let mut changes = Vec::new();
    let mut push = |code: &str| {
        if changes.len() < MAX_VERIFY_CHANGE_CODES {
            changes.push(code.to_owned());
        }
    };
    if before.role != after.role {
        push("role_changed");
    }
    if before.name != after.name {
        push("name_changed");
    }
    if before.tag != after.tag {
        push("tag_changed");
    }
    if before.interactive != after.interactive {
        push("interactive_changed");
    }
    if before.attributes != after.attributes {
        push("attributes_changed");
    }
    if before.source != after.source {
        push("source_locator_changed");
    }
    if before.rect != after.rect {
        push("geometry_changed");
    }
    changes
}

pub fn compare_issue_fingerprints(
    before_console: &[VerifyConsoleFingerprint],
    after_console: &[VerifyConsoleFingerprint],
    before_network: &[VerifyNetworkFingerprint],
    after_network: &[VerifyNetworkFingerprint],
) -> Vec<String> {
    let mut regressions = Vec::new();
    let before_console_map = before_console
        .iter()
        .map(|issue| ((issue.level.clone(), issue.message.clone()), issue.count))
        .collect::<BTreeMap<_, _>>();
    for issue in after_console {
        if !issue.level.eq_ignore_ascii_case("error") {
            continue;
        }
        let previous = before_console_map
            .get(&(issue.level.clone(), issue.message.clone()))
            .copied()
            .unwrap_or(0);
        if issue.count > previous && regressions.len() < MAX_VERIFY_REGRESSION_CODES {
            regressions.push("new_console_error".to_owned());
            break;
        }
    }

    let before_network_set = before_network
        .iter()
        .map(|issue| {
            (
                issue.method.clone(),
                issue.path.clone(),
                issue.status,
                issue.error.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    if after_network.iter().any(|issue| {
        !before_network_set.contains(&(
            issue.method.clone(),
            issue.path.clone(),
            issue.status,
            issue.error.clone(),
        ))
    }) && regressions.len() < MAX_VERIFY_REGRESSION_CODES
    {
        regressions.push("new_network_failure".to_owned());
    }
    regressions
}

fn crop_rgba(image: &RgbaImage, left: u32, top: u32, right: u32, bottom: u32) -> Option<RgbaImage> {
    if left >= right || top >= bottom || right > image.width || bottom > image.height {
        return None;
    }
    let width = right - left;
    let height = bottom - top;
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for y in top..bottom {
        let start = ((y * image.width + left) * 4) as usize;
        let end = ((y * image.width + right) * 4) as usize;
        data.extend_from_slice(&image.data[start..end]);
    }
    let cropped = RgbaImage {
        width,
        height,
        data,
    };
    cropped.validate().ok()?;
    Some(cropped)
}

fn contains_rect(container: &Rect, child: &Rect) -> bool {
    let values = [
        container.x,
        container.y,
        container.width,
        container.height,
        child.x,
        child.y,
        child.width,
        child.height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || container.width <= 0.0
        || container.height <= 0.0
        || child.width <= 0.0
        || child.height <= 0.0
    {
        return false;
    }
    let container_right = container.x + container.width;
    let container_bottom = container.y + container.height;
    let child_right = child.x + child.width;
    let child_bottom = child.y + child.height;
    container_right.is_finite()
        && container_bottom.is_finite()
        && child_right.is_finite()
        && child_bottom.is_finite()
        && container.x <= child.x
        && container.y <= child.y
        && container_right >= child_right
        && container_bottom >= child_bottom
}

fn target_union_pixels(
    before_rect: &Rect,
    after_rect: &Rect,
    viewport: &ViewportMeta,
    image: &RgbaImage,
) -> Option<(u32, u32, u32, u32)> {
    let values = [
        before_rect.x,
        before_rect.y,
        before_rect.width,
        before_rect.height,
        after_rect.x,
        after_rect.y,
        after_rect.width,
        after_rect.height,
    ];
    if values.iter().any(|value| !value.is_finite())
        || viewport.css_width == 0
        || viewport.css_height == 0
    {
        return None;
    }

    let x0 = before_rect.x.min(after_rect.x).max(0.0);
    let y0 = before_rect.y.min(after_rect.y).max(0.0);
    let x1 = (before_rect.x + before_rect.width)
        .max(after_rect.x + after_rect.width)
        .min(viewport.css_width as f64);
    let y1 = (before_rect.y + before_rect.height)
        .max(after_rect.y + after_rect.height)
        .min(viewport.css_height as f64);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let sx = image.width as f64 / viewport.css_width as f64;
    let sy = image.height as f64 / viewport.css_height as f64;
    let left = (x0 * sx).floor().max(0.0) as u32;
    let top = (y0 * sy).floor().max(0.0) as u32;
    let right = (x1 * sx).ceil().min(image.width as f64) as u32;
    let bottom = (y1 * sy).ceil().min(image.height as f64) as u32;
    Some((left, top, right, bottom))
}

pub fn compare_visual_facts(
    before: &VerifyVisualBaseline,
    after_png: &[u8],
    after_viewport: &ViewportMeta,
    after_rect: Option<&Rect>,
) -> Result<VisualVerificationFacts, String> {
    if before.png.len() > MAX_VERIFY_VISUAL_BYTES_PER_RECORD
        || after_png.len() > MAX_VERIFY_VISUAL_BYTES_PER_RECORD
    {
        return Err("trusted Verify visual frame exceeds safety bound".into());
    }
    if before.viewport != *after_viewport {
        return Ok(VisualVerificationFacts {
            viewport_changed_ratio: None,
            target_changed_ratio: None,
            affected_region_changed_ratio: None,
        });
    }
    let before_image = decode_png_rgba(before.png.as_slice())
        .map_err(|_| "trusted Verify visual baseline decode failed".to_string())?;
    let after_image = decode_png_rgba(after_png)
        .map_err(|_| "trusted Verify current visual decode failed".to_string())?;
    if (before_image.width, before_image.height) != (after_image.width, after_image.height) {
        return Ok(VisualVerificationFacts {
            viewport_changed_ratio: None,
            target_changed_ratio: None,
            affected_region_changed_ratio: None,
        });
    }

    if let Some(capture_region) = before.capture_region.as_ref() {
        let target_still_covered = match (before.target_rect.as_ref(), after_rect) {
            (Some(before_rect), Some(after_rect)) => {
                contains_rect(capture_region, before_rect)
                    && contains_rect(capture_region, after_rect)
            }
            _ => false,
        };
        let affected_region_changed_ratio = if target_still_covered {
            Some(
                pixel_diff(&before_image, &after_image, VERIFY_PIXEL_THRESHOLD)
                    .map_err(|_| "trusted Verify affected-region diff failed".to_string())?
                    .changed_ratio,
            )
        } else {
            None
        };
        return Ok(VisualVerificationFacts {
            viewport_changed_ratio: None,
            target_changed_ratio: None,
            affected_region_changed_ratio,
        });
    }

    let viewport_diff = pixel_diff(&before_image, &after_image, VERIFY_PIXEL_THRESHOLD)
        .map_err(|_| "trusted Verify viewport diff failed".to_string())?;

    let target_changed_ratio = match (before.target_rect.as_ref(), after_rect) {
        (Some(before_rect), Some(after_rect)) => {
            target_union_pixels(before_rect, after_rect, &before.viewport, &before_image).and_then(
                |(left, top, right, bottom)| {
                    let before_crop = crop_rgba(&before_image, left, top, right, bottom)?;
                    let after_crop = crop_rgba(&after_image, left, top, right, bottom)?;
                    pixel_diff(&before_crop, &after_crop, VERIFY_PIXEL_THRESHOLD)
                        .ok()
                        .map(|diff| diff.changed_ratio)
                },
            )
        }
        _ => None,
    };

    Ok(VisualVerificationFacts {
        viewport_changed_ratio: Some(viewport_diff.changed_ratio),
        target_changed_ratio,
        affected_region_changed_ratio: None,
    })
}

pub fn classify_verification_status(
    semantic_changes: Vec<String>,
    mut regression_signals: Vec<String>,
    visual: &VisualVerificationFacts,
    scope: VerificationScope,
    before_interactive: bool,
    after_interactive: bool,
) -> VerificationComparison {
    if before_interactive
        && !after_interactive
        && regression_signals.len() < MAX_VERIFY_REGRESSION_CODES
    {
        regression_signals.push("target_became_non_interactive".to_owned());
    }
    regression_signals.sort();
    regression_signals.dedup();

    let target_visual_changed = visual
        .target_changed_ratio
        .map(|ratio| ratio > 0.0)
        .unwrap_or(false);
    let affected_region_visual_changed = visual
        .affected_region_changed_ratio
        .map(|ratio| ratio > 0.0)
        .unwrap_or(false);
    let viewport_visual_changed = visual
        .viewport_changed_ratio
        .map(|ratio| ratio > 0.0)
        .unwrap_or(false);
    let semantic_changed = !semantic_changes.is_empty();
    let visual_available = visual.viewport_changed_ratio.is_some()
        || visual.target_changed_ratio.is_some()
        || visual.affected_region_changed_ratio.is_some();

    let deterministic_status = if !regression_signals.is_empty() {
        DeterministicVerificationStatus::RegressionSignal
    } else if semantic_changed || target_visual_changed || affected_region_visual_changed {
        DeterministicVerificationStatus::ChangeObserved
    } else if scope == VerificationScope::SemanticVisual && !visual_available {
        DeterministicVerificationStatus::Inconclusive
    } else if scope == VerificationScope::SemanticVisual
        && viewport_visual_changed
        && !target_visual_changed
        && visual.affected_region_changed_ratio.is_none()
    {
        DeterministicVerificationStatus::Inconclusive
    } else {
        DeterministicVerificationStatus::NoObservableChange
    };

    VerificationComparison {
        deterministic_status,
        semantic_changes,
        regression_signals,
        viewport_changed_ratio: visual.viewport_changed_ratio,
        target_changed_ratio: visual.target_changed_ratio,
        affected_region_changed_ratio: visual.affected_region_changed_ratio,
    }
}

pub fn mint_verification_baseline(
    store: &VerificationStore,
    proposal_id: &str,
    session: &Session,
    snapshot: &PageSnapshot,
    reference: &str,
    canonical_route: &str,
    canonical_file: std::path::PathBuf,
    project_root: std::path::PathBuf,
    display_file: String,
    source_line: u32,
    postimage: Vec<u8>,
    instruction: String,
    visual_before: Option<VerifyVisualBaseline>,
) -> Result<(String, VerificationScope), String> {
    let semantic_before = build_semantic_baseline(session, snapshot, reference)?;
    let verification_id = format!("lvv-{}", Uuid::new_v4());
    let scope = if visual_before.is_some() {
        VerificationScope::SemanticVisual
    } else {
        VerificationScope::SemanticOnly
    };
    let record = VerificationRecord {
        verification_id: verification_id.clone(),
        proposal_id: proposal_id.to_owned(),
        session_id: session.id,
        reference: reference.to_owned(),
        canonical_route: canonical_route.to_owned(),
        canonical_file,
        project_root,
        display_file,
        source_line,
        postimage,
        instruction,
        semantic_before,
        visual_before,
        scope,
        created_at: Instant::now(),
        expires_at: Instant::now() + VERIFICATION_TTL,
        status: VerificationStatus::Pending,
    };
    store.insert(record)?;
    Ok((verification_id, scope))
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod trusted_verify_tests {
    use super::*;
    use chrono::Utc;
    use localview_protocol::{
        Classification, ConsoleIssue, Endpoint, NetworkIssue, ProjectIdentity, ServerKind,
        SessionStatus, SourceLocation,
    };

    fn node(reference: &str, name: &str, interactive: bool, rect: Option<Rect>) -> SemanticNode {
        SemanticNode {
            reference: reference.to_owned(),
            role: Some("button".into()),
            name: Some(name.into()),
            tag: "button".into(),
            rect,
            interactive,
            attributes: BTreeMap::from([
                ("id".into(), "deploy".into()),
                ("value".into(), "SECRET".into()),
            ]),
            source: Some(SourceLocation {
                file: "src/App.tsx".into(),
                line: 4,
                column: Some(3),
                component: Some("App".into()),
            }),
            ownership: None,
            children: Vec::new(),
        }
    }

    fn session() -> Session {
        Session {
            id: Uuid::new_v4(),
            endpoint: Endpoint {
                host: "127.0.0.1".into(),
                port: 5173,
                scheme: "http".into(),
            },
            classification: Classification {
                kind: ServerKind::FrontendDevServer,
                confidence: 1.0,
                framework: Some("React".into()),
                title: Some("Fixture".into()),
                hmr_detected: true,
                ..Classification::default()
            },
            project: ProjectIdentity {
                key: "fixture".into(),
                display_name: "Fixture".into(),
                cwd: Some("/private/fixture".into()),
                git_root: Some("/private/fixture".into()),
                pid: None,
                command: None,
            },
            status: SessionStatus::Active,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
            disconnected_at: None,
            preview_visible: true,
        }
    }

    fn snapshot(selected: SemanticNode) -> PageSnapshot {
        PageSnapshot {
            version: 3,
            route: "http://127.0.0.1:5173/settings?token=secret#hidden".into(),
            viewport: (100, 100),
            root: SemanticNode {
                reference: "@e0".into(),
                role: Some("document".into()),
                name: None,
                tag: "body".into(),
                rect: Some(Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                }),
                interactive: false,
                attributes: BTreeMap::new(),
                source: None,
                ownership: None,
                children: vec![selected],
            },
            console_errors: vec![ConsoleIssue {
                level: "error".into(),
                message: "boom".into(),
                source: None,
                count: 1,
            }],
            failed_requests: vec![NetworkIssue {
                method: "GET".into(),
                url: "https://example.test/api/items?secret=1".into(),
                status: Some(500),
                error: Some("failed".into()),
            }],
            captured_at: Utc::now(),
        }
    }

    fn dummy_record(visual_bytes: usize) -> VerificationRecord {
        let s = session();
        let snap = snapshot(node(
            "@e1",
            "Deploy",
            true,
            Some(Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 10.0,
            }),
        ));
        let semantic = build_semantic_baseline(&s, &snap, "@e1").unwrap();
        VerificationRecord {
            verification_id: format!("lvv-{}", Uuid::new_v4()),
            proposal_id: format!("lvp-{}", Uuid::new_v4()),
            session_id: s.id,
            reference: "@e1".into(),
            canonical_route: "http://127.0.0.1:5173/settings".into(),
            canonical_file: "src/App.tsx".into(),
            project_root: ".".into(),
            display_file: "src/App.tsx".into(),
            source_line: 4,
            postimage: b"after".to_vec(),
            instruction: "make it clearer".into(),
            semantic_before: semantic,
            visual_before: (visual_bytes > 0).then(|| VerifyVisualBaseline {
                png: Arc::new(vec![0; visual_bytes]),
                viewport: ViewportMeta {
                    css_width: 100,
                    css_height: 100,
                    device_scale_factor: 1.0,
                },
                pixel_width: 100,
                pixel_height: 100,
                target_rect: None,
                capture_region: None,
                captured_at_unix_ms: 1,
            }),
            scope: if visual_bytes > 0 {
                VerificationScope::SemanticVisual
            } else {
                VerificationScope::SemanticOnly
            },
            created_at: Instant::now(),
            expires_at: Instant::now() + VERIFICATION_TTL,
            status: VerificationStatus::Pending,
        }
    }

    #[test]
    fn trusted_verify_semantic_baseline_reuses_redacted_ai_projection() {
        let s = session();
        let snap = snapshot(node(
            "@e1",
            "Deploy",
            true,
            Some(Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 10.0,
            }),
        ));
        let baseline = build_semantic_baseline(&s, &snap, "@e1").unwrap();
        let encoded = serde_json::to_string(&baseline).unwrap();
        assert!(encoded.contains("src/App.tsx:4:3"));
        assert!(!encoded.contains("SECRET"));
        assert!(!encoded.contains("/private/fixture"));
        assert!(!encoded.contains("secret=1"));
    }

    #[test]
    fn trusted_verify_store_is_bounded_one_shot_and_retryable() {
        let store = VerificationStore::default();
        let record = dummy_record(0);
        let id = record.verification_id.clone();
        store.insert(record).unwrap();
        let begun = store.begin_verify(&id).unwrap();
        assert_eq!(begun.status, VerificationStatus::Verifying);
        assert!(store.begin_verify(&id).is_err());
        store.release_retryable(&id).unwrap();
        assert!(store.begin_verify(&id).is_ok());
        store.complete(&id).unwrap();
        assert!(store.begin_verify(&id).is_err());
        assert_eq!(store.retained_visual_bytes_for_test(), 0);
    }

    #[test]
    fn trusted_verify_store_rejects_visual_budget_overflow() {
        let store = VerificationStore::default();
        let oversized = dummy_record(MAX_VERIFY_VISUAL_BYTES_PER_RECORD + 1);
        assert!(store.insert(oversized).is_err());
        assert_eq!(store.retained_visual_bytes_for_test(), 0);
    }

    #[test]
    fn trusted_verify_semantic_and_issue_comparison_is_deterministic() {
        let s = session();
        let before_snapshot = snapshot(node("@e1", "Deploy", true, None));
        let mut after_snapshot = snapshot(node("@e1", "Publish", false, None));
        after_snapshot.console_errors[0].count = 2;
        after_snapshot.failed_requests.push(NetworkIssue {
            method: "POST".into(),
            url: "https://example.test/api/save?token=x".into(),
            status: Some(500),
            error: Some("failed".into()),
        });
        let before = build_semantic_baseline(&s, &before_snapshot, "@e1").unwrap();
        let after = build_semantic_baseline(&s, &after_snapshot, "@e1").unwrap();

        let changes = compare_semantic_projection(&before.selected, &after.selected);
        assert_eq!(changes, vec!["name_changed", "interactive_changed"]);

        let regressions = compare_issue_fingerprints(
            &before.console_issues,
            &after.console_issues,
            &before.network_issues,
            &after.network_issues,
        );
        assert_eq!(
            regressions,
            vec!["new_console_error", "new_network_failure"]
        );
    }

    #[test]
    fn semantic_visual_scope_without_current_visual_facts_is_inconclusive() {
        let comparison = classify_verification_status(
            Vec::new(),
            Vec::new(),
            &VisualVerificationFacts {
                viewport_changed_ratio: None,
                target_changed_ratio: None,
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticVisual,
            true,
            true,
        );
        assert_eq!(
            comparison.deterministic_status,
            DeterministicVerificationStatus::Inconclusive
        );
    }

    #[test]
    fn trusted_verify_affected_region_diff_is_distinct_from_viewport_and_exact_target() {
        let viewport = ViewportMeta {
            css_width: 100,
            css_height: 100,
            device_scale_factor: 1.0,
        };
        let target = Rect {
            x: 20.0,
            y: 20.0,
            width: 10.0,
            height: 10.0,
        };
        let capture_region = Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        };
        let before_image = solid_image(50, 50, [255, 255, 255, 255]);
        let mut after_image = before_image.clone();
        set_pixel(&mut after_image, 1, 1, [0, 0, 0, 255]);

        let mut before = visual_baseline(&before_image, viewport.clone(), Some(target.clone()));
        before.capture_region = Some(capture_region);
        let after_png = localview_visual::encode_png_rgba(&after_image).unwrap();
        let facts =
            compare_visual_facts(&before, &after_png, &viewport, Some(&target)).unwrap();

        assert_eq!(facts.viewport_changed_ratio, None);
        assert_eq!(facts.target_changed_ratio, None);
        assert!(facts.affected_region_changed_ratio.is_some_and(|ratio| ratio > 0.0));

        let comparison = classify_verification_status(
            Vec::new(),
            Vec::new(),
            &facts,
            VerificationScope::SemanticVisual,
            true,
            true,
        );
        assert_eq!(
            comparison.deterministic_status,
            DeterministicVerificationStatus::ChangeObserved
        );
    }

    #[test]
    fn trusted_verify_affected_region_refuses_visual_claim_when_target_escapes_region() {
        let viewport = ViewportMeta {
            css_width: 100,
            css_height: 100,
            device_scale_factor: 1.0,
        };
        let before_target = Rect {
            x: 10.0,
            y: 10.0,
            width: 10.0,
            height: 10.0,
        };
        let after_target = Rect {
            x: 80.0,
            y: 80.0,
            width: 10.0,
            height: 10.0,
        };
        let image = solid_image(40, 40, [255, 255, 255, 255]);
        let mut before =
            visual_baseline(&image, viewport.clone(), Some(before_target));
        before.capture_region = Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        });
        let after_png = localview_visual::encode_png_rgba(&image).unwrap();
        let facts =
            compare_visual_facts(&before, &after_png, &viewport, Some(&after_target)).unwrap();

        assert_eq!(facts.viewport_changed_ratio, None);
        assert_eq!(facts.target_changed_ratio, None);
        assert_eq!(facts.affected_region_changed_ratio, None);
    }

    #[test]
    fn trusted_verify_visual_compare_rejects_oversized_current_frame_before_decode() {
        let before = VerifyVisualBaseline {
            png: Arc::new(vec![0; 1]),
            viewport: ViewportMeta {
                css_width: 100,
                css_height: 100,
                device_scale_factor: 1.0,
            },
            pixel_width: 100,
            pixel_height: 100,
            target_rect: None,
            capture_region: None,
            captured_at_unix_ms: 1,
        };
        let oversized = vec![0; MAX_VERIFY_VISUAL_BYTES_PER_RECORD + 1];
        let error = compare_visual_facts(&before, &oversized, &before.viewport, None).unwrap_err();
        assert!(error.contains("exceeds safety bound"));
    }

    #[test]
    fn trusted_verify_status_prioritizes_regressions_then_target_change() {
        let clean = classify_verification_status(
            vec!["name_changed".into()],
            Vec::new(),
            &VisualVerificationFacts {
                viewport_changed_ratio: Some(0.2),
                target_changed_ratio: Some(0.1),
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticVisual,
            true,
            true,
        );
        assert_eq!(
            clean.deterministic_status,
            DeterministicVerificationStatus::ChangeObserved
        );

        let regression = classify_verification_status(
            Vec::new(),
            vec!["new_console_error".into()],
            &VisualVerificationFacts {
                viewport_changed_ratio: Some(0.0),
                target_changed_ratio: Some(0.0),
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticVisual,
            true,
            true,
        );
        assert_eq!(
            regression.deterministic_status,
            DeterministicVerificationStatus::RegressionSignal
        );

        let outside_only = classify_verification_status(
            Vec::new(),
            Vec::new(),
            &VisualVerificationFacts {
                viewport_changed_ratio: Some(0.3),
                target_changed_ratio: Some(0.0),
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticVisual,
            true,
            true,
        );
        assert_eq!(
            outside_only.deterministic_status,
            DeterministicVerificationStatus::Inconclusive
        );

        let semantic_only_unchanged = classify_verification_status(
            Vec::new(),
            Vec::new(),
            &VisualVerificationFacts {
                viewport_changed_ratio: None,
                target_changed_ratio: None,
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticOnly,
            true,
            true,
        );
        assert_eq!(
            semantic_only_unchanged.deterministic_status,
            DeterministicVerificationStatus::NoObservableChange
        );
    }

    fn solid_image(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
        let mut data = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            data.extend_from_slice(&rgba);
        }
        RgbaImage {
            width,
            height,
            data,
        }
    }

    fn set_pixel(image: &mut RgbaImage, x: u32, y: u32, rgba: [u8; 4]) {
        let offset = ((y * image.width + x) * 4) as usize;
        image.data[offset..offset + 4].copy_from_slice(&rgba);
    }

    fn visual_baseline(
        image: &RgbaImage,
        viewport: ViewportMeta,
        target_rect: Option<Rect>,
    ) -> VerifyVisualBaseline {
        VerifyVisualBaseline {
            png: Arc::new(localview_visual::encode_png_rgba(image).unwrap()),
            viewport,
            pixel_width: image.width,
            pixel_height: image.height,
            target_rect,
            capture_region: None,
            captured_at_unix_ms: 1,
        }
    }

    #[test]
    fn trusted_verify_store_enforces_capacity_total_budget_and_cleanup() {
        let capacity_store = VerificationStore::default();
        for _ in 0..MAX_VERIFICATION_RECORDS {
            capacity_store.insert(dummy_record(0)).unwrap();
        }
        let capacity_error = capacity_store.insert(dummy_record(0)).unwrap_err();
        assert!(capacity_error.contains("capacity exceeded"));

        let budget_store = VerificationStore::default();
        for _ in 0..4 {
            budget_store
                .insert(dummy_record(MAX_VERIFY_VISUAL_BYTES_PER_RECORD))
                .unwrap();
        }
        assert_eq!(
            budget_store.retained_visual_bytes_for_test(),
            MAX_VERIFY_TOTAL_VISUAL_BYTES
        );
        let budget_error = budget_store.insert(dummy_record(1)).unwrap_err();
        assert!(budget_error.contains("budget exceeded"));

        let cleanup_store = VerificationStore::default();
        let record = dummy_record(1024);
        let id = record.verification_id.clone();
        cleanup_store.insert(record).unwrap();
        assert_eq!(cleanup_store.retained_visual_bytes_for_test(), 1024);
        cleanup_store.invalidate(&id).unwrap();
        assert_eq!(cleanup_store.retained_visual_bytes_for_test(), 0);
        assert!(cleanup_store.begin_verify(&id).is_err());
    }

    #[test]
    fn trusted_verify_store_reaps_expired_records_and_visual_bytes() {
        let store = VerificationStore::default();
        let mut record = dummy_record(2048);
        let id = record.verification_id.clone();
        record.expires_at = Instant::now() - Duration::from_millis(1);
        store.insert(record).unwrap();
        assert_eq!(store.retained_visual_bytes_for_test(), 2048);

        store.reap_expired().unwrap();

        assert_eq!(store.retained_visual_bytes_for_test(), 0);
        assert!(store.begin_verify(&id).is_err());
    }

    #[test]
    fn trusted_verify_semantic_projection_order_and_target_identity_are_locked() {
        let s = session();
        let base_snapshot = snapshot(node(
            "@e1",
            "Deploy",
            true,
            Some(Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 10.0,
            }),
        ));
        let before = build_semantic_baseline(&s, &base_snapshot, "@e1").unwrap();
        assert!(compare_semantic_projection(&before.selected, &before.selected).is_empty());

        let mut after = before.selected.clone();
        after.role = Some("link".into());
        after.name = Some("Publish".into());
        after.interactive = false;
        after
            .attributes
            .insert("aria-label".into(), "Publish".into());
        after.rect = Some(Rect {
            x: 11.0,
            y: 10.0,
            width: 20.0,
            height: 10.0,
        });
        assert_eq!(
            compare_semantic_projection(&before.selected, &after),
            vec![
                "role_changed",
                "name_changed",
                "interactive_changed",
                "attributes_changed",
                "geometry_changed",
            ]
        );

        let missing = snapshot(node("@other", "Other", true, None));
        assert!(build_semantic_baseline(&s, &missing, "@e1").is_err());

        let mut duplicate = base_snapshot.clone();
        duplicate.root.children.push(node(
            "@e1",
            "Duplicate",
            true,
            Some(Rect {
                x: 40.0,
                y: 10.0,
                width: 20.0,
                height: 10.0,
            }),
        ));
        let duplicate_error = build_semantic_baseline(&s, &duplicate, "@e1").unwrap_err();
        assert!(duplicate_error.contains("ambiguous"));
    }

    #[test]
    fn trusted_verify_issue_fingerprints_are_bounded_and_unchanged_is_clean() {
        let s = session();
        let mut snap = snapshot(node("@e1", "Deploy", true, None));
        snap.console_errors = (0..32)
            .map(|index| ConsoleIssue {
                level: "error".into(),
                message: format!("console-{index}"),
                source: None,
                count: 1,
            })
            .collect();
        snap.failed_requests = (0..32)
            .map(|index| NetworkIssue {
                method: "GET".into(),
                url: format!("https://example.test/api/{index}?secret=hidden"),
                status: Some(500),
                error: Some("failed".into()),
            })
            .collect();

        let baseline = build_semantic_baseline(&s, &snap, "@e1").unwrap();
        assert_eq!(
            baseline.console_issues.len(),
            trusted_ai::MAX_AI_CONSOLE_ISSUES
        );
        assert_eq!(
            baseline.network_issues.len(),
            trusted_ai::MAX_AI_NETWORK_ISSUES
        );
        assert!(
            baseline
                .network_issues
                .iter()
                .all(|issue| !issue.path.contains('?') && !issue.path.contains("secret"))
        );

        assert!(
            compare_issue_fingerprints(
                &baseline.console_issues,
                &baseline.console_issues,
                &baseline.network_issues,
                &baseline.network_issues,
            )
            .is_empty()
        );
    }

    #[test]
    fn trusted_verify_visual_facts_cover_target_outside_dimension_and_threshold_cases() {
        let viewport = ViewportMeta {
            css_width: 4,
            css_height: 4,
            device_scale_factor: 1.0,
        };
        let target = Rect {
            x: 0.0,
            y: 0.0,
            width: 2.0,
            height: 2.0,
        };
        let before_image = solid_image(4, 4, [0, 0, 0, 255]);
        let before = visual_baseline(&before_image, viewport.clone(), Some(target.clone()));
        let identical_png = localview_visual::encode_png_rgba(&before_image).unwrap();

        let identical =
            compare_visual_facts(&before, &identical_png, &viewport, Some(&target)).unwrap();
        assert_eq!(identical.viewport_changed_ratio, Some(0.0));
        assert_eq!(identical.target_changed_ratio, Some(0.0));

        let mut target_changed_image = before_image.clone();
        set_pixel(&mut target_changed_image, 1, 1, [255, 255, 255, 255]);
        let target_changed_png = localview_visual::encode_png_rgba(&target_changed_image).unwrap();
        let target_changed =
            compare_visual_facts(&before, &target_changed_png, &viewport, Some(&target)).unwrap();
        assert!(target_changed.viewport_changed_ratio.unwrap() > 0.0);
        assert!(target_changed.target_changed_ratio.unwrap() > 0.0);

        let mut outside_changed_image = before_image.clone();
        set_pixel(&mut outside_changed_image, 3, 3, [255, 255, 255, 255]);
        let outside_changed_png =
            localview_visual::encode_png_rgba(&outside_changed_image).unwrap();
        let outside_changed =
            compare_visual_facts(&before, &outside_changed_png, &viewport, Some(&target)).unwrap();
        assert!(outside_changed.viewport_changed_ratio.unwrap() > 0.0);
        assert_eq!(outside_changed.target_changed_ratio, Some(0.0));

        let different_size = solid_image(5, 4, [0, 0, 0, 255]);
        let different_size_png = localview_visual::encode_png_rgba(&different_size).unwrap();
        let dimension_mismatch =
            compare_visual_facts(&before, &different_size_png, &viewport, Some(&target)).unwrap();
        assert_eq!(dimension_mismatch.viewport_changed_ratio, None);
        assert_eq!(dimension_mismatch.target_changed_ratio, None);

        let invalid_target = Rect {
            x: f64::NAN,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let invalid_target_facts =
            compare_visual_facts(&before, &identical_png, &viewport, Some(&invalid_target))
                .unwrap();
        assert_eq!(invalid_target_facts.viewport_changed_ratio, Some(0.0));
        assert_eq!(invalid_target_facts.target_changed_ratio, None);

        let mut threshold_equal_image = before_image.clone();
        set_pixel(
            &mut threshold_equal_image,
            0,
            0,
            [VERIFY_PIXEL_THRESHOLD, 0, 0, 255],
        );
        let threshold_equal_png =
            localview_visual::encode_png_rgba(&threshold_equal_image).unwrap();
        let threshold_equal =
            compare_visual_facts(&before, &threshold_equal_png, &viewport, Some(&target)).unwrap();

        let mut threshold_exceeded_image = before_image.clone();
        set_pixel(
            &mut threshold_exceeded_image,
            0,
            0,
            [VERIFY_PIXEL_THRESHOLD.saturating_add(1), 0, 0, 255],
        );
        let threshold_exceeded_png =
            localview_visual::encode_png_rgba(&threshold_exceeded_image).unwrap();
        let threshold_exceeded =
            compare_visual_facts(&before, &threshold_exceeded_png, &viewport, Some(&target))
                .unwrap();

        let equal_ratio = threshold_equal.viewport_changed_ratio.unwrap();
        let exceeded_ratio = threshold_exceeded.viewport_changed_ratio.unwrap();
        assert!((0.0..=1.0).contains(&equal_ratio));
        assert!((0.0..=1.0).contains(&exceeded_ratio));
        assert!(exceeded_ratio >= equal_ratio);
    }

    #[test]
    fn trusted_verify_status_covers_semantic_only_change_and_visual_no_change() {
        let visual_no_change = classify_verification_status(
            Vec::new(),
            Vec::new(),
            &VisualVerificationFacts {
                viewport_changed_ratio: Some(0.0),
                target_changed_ratio: Some(0.0),
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticVisual,
            true,
            true,
        );
        assert_eq!(
            visual_no_change.deterministic_status,
            DeterministicVerificationStatus::NoObservableChange
        );

        let semantic_only_change = classify_verification_status(
            vec!["name_changed".into()],
            Vec::new(),
            &VisualVerificationFacts {
                viewport_changed_ratio: None,
                target_changed_ratio: None,
                affected_region_changed_ratio: None,
            },
            VerificationScope::SemanticOnly,
            true,
            true,
        );
        assert_eq!(
            semantic_only_change.deterministic_status,
            DeterministicVerificationStatus::ChangeObserved
        );
    }
}
