use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use localview_counterfactual::sha256_bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::trusted_verify::{
    MAX_VERIFICATION_RECORDS, MAX_VERIFY_TOTAL_VISUAL_BYTES, MAX_VERIFY_VISUAL_BYTES_PER_RECORD,
    VERIFY_CONTEXT_VERSION, VerificationRecord, VerificationScope, VerificationStatus,
    VerifySemanticBaseline, VerifyVisualBaseline, now_unix_ms,
};

const RECOVERY_SCHEMA_VERSION: u32 = 1;
const WAVE9_RECOVERY_SCHEMA_VERSION: u32 = 1;
const RECOVERY_DIR: &str = "trusted-verify-v1";
const MAX_RECOVERY_METADATA_BYTES: usize = 256 * 1024;
const MAX_RECOVERY_WAVE9_BYTES: usize = 256 * 1024;

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

#[derive(Debug, Default)]
pub(crate) enum VerificationPersistence {
    #[default]
    Disabled,
    Ready { root: PathBuf },
    Unavailable { reason: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedVerificationRecordV1 {
    schema_version: u32,
    verification_id: String,
    proposal_id: String,
    session_id: localview_protocol::SessionId,
    reference: String,
    canonical_route: String,
    canonical_file: PathBuf,
    project_root: PathBuf,
    display_file: String,
    source_line: u32,
    postimage_sha256: String,
    instruction: String,
    semantic_before: VerifySemanticBaseline,
    visual_before: Option<PersistedVisualBaselineV1>,
    scope: VerificationScope,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedVerificationRecordV2 {
    schema_version: u32,
    verification_id: String,
    proposal_id: String,
    session_id: localview_protocol::SessionId,
    reference: String,
    canonical_route: String,
    canonical_file: PathBuf,
    project_root: PathBuf,
    display_file: String,
    source_line: u32,
    postimage_sha256: String,
    instruction: String,
    semantic_before: VerifySemanticBaseline,
    visual_before: Option<PersistedVisualBaselineV1>,
    scope: VerificationScope,
    wave9_preflight: Option<localview_verification::ProductionCandidatePreflightReceipt>,
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedWave9PreflightV1 {
    schema_version: u32,
    verification_id: String,
    wave9_preflight: localview_verification::ProductionCandidatePreflightReceipt,
}

impl From<PersistedVerificationRecordV1> for PersistedVerificationRecordV2 {
    fn from(record: PersistedVerificationRecordV1) -> Self {
        Self {
            schema_version: RECOVERY_SCHEMA_VERSION,
            verification_id: record.verification_id,
            proposal_id: record.proposal_id,
            session_id: record.session_id,
            reference: record.reference,
            canonical_route: record.canonical_route,
            canonical_file: record.canonical_file,
            project_root: record.project_root,
            display_file: record.display_file,
            source_line: record.source_line,
            postimage_sha256: record.postimage_sha256,
            instruction: record.instruction,
            semantic_before: record.semantic_before,
            visual_before: record.visual_before,
            scope: record.scope,
            wave9_preflight: None,
            created_at_unix_ms: record.created_at_unix_ms,
            expires_at_unix_ms: record.expires_at_unix_ms,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedVisualBaselineV1 {
    png_sha256: String,
    viewport: localview_native_capture::ViewportMeta,
    pixel_width: u32,
    pixel_height: u32,
    target_rect: Option<localview_protocol::Rect>,
    captured_at_unix_ms: u64,
}


impl VerificationPersistence {
    pub(crate) fn production_default() -> (Self, HashMap<String, VerificationRecord>) {
        let Some(data_root) = dirs::data_local_dir() else {
            return (
                Self::Unavailable {
                    reason: "no local data directory".into(),
                },
                HashMap::new(),
            );
        };
        let localview_root = data_root.join("LocalView");
        let recovery_root = localview_root.join(RECOVERY_DIR);
        match open_recovery_root(&localview_root, &recovery_root)
            .and_then(|_| load_records(&recovery_root))
        {
            Ok(records) => (
                Self::Ready {
                    root: recovery_root,
                },
                records,
            ),
            Err(reason) => (Self::Unavailable { reason }, HashMap::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn open_at(
        root: PathBuf,
    ) -> Result<(Self, HashMap<String, VerificationRecord>), String> {
        ensure_private_directory(&root)?;
        let records = load_records(&root)?;
        Ok((Self::Ready { root }, records))
    }

    pub(crate) fn persist(&self, record: &VerificationRecord) -> Result<(), String> {
        match self {
            Self::Disabled => Ok(()),
            Self::Unavailable { reason } => Err(format!(
                "trusted Verify durable recovery unavailable: {reason}"
            )),
            Self::Ready { root } => persist_record(root, record),
        }
    }

    pub(crate) fn consume(&self, verification_id: &str) -> Result<(), String> {
        match self {
            Self::Disabled => Ok(()),
            Self::Unavailable { reason } => Err(format!(
                "trusted Verify durable recovery unavailable: {reason}"
            )),
            Self::Ready { root } => consume_record(root, verification_id),
        }
    }
}

fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn ensure_private_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || metadata_is_reparse_point(&metadata)
                || !metadata.is_dir() =>
        {
            return Err(format!(
                "trusted Verify recovery path is not a real directory: {}",
                path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder
                    .create(path)
                    .map_err(|error| format!("create trusted Verify recovery directory: {error}"))?;
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                    .map_err(|error| format!("secure trusted Verify recovery directory: {error}"))?;
            }
            #[cfg(not(unix))]
            fs::create_dir(path)
                .map_err(|error| format!("create trusted Verify recovery directory: {error}"))?;
        }
        Err(error) => {
            return Err(format!(
                "inspect trusted Verify recovery directory {}: {error}",
                path.display()
            ));
        }
    }

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("reinspect trusted Verify recovery directory: {error}"))?;
    if metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || !metadata.is_dir()
    {
        return Err("trusted Verify recovery directory identity is unsafe".into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("secure trusted Verify recovery directory: {error}"))?;
        let mode = fs::metadata(path)
            .map_err(|error| format!("inspect trusted Verify recovery permissions: {error}"))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err("trusted Verify recovery directory is not owner-only".into());
        }
    }

    Ok(())
}

fn open_recovery_root(localview_root: &Path, recovery_root: &Path) -> Result<(), String> {
    if !localview_root.exists() {
        if let Some(parent) = localview_root.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create LocalView data parent: {error}"))?;
        }
    }
    ensure_private_directory(localview_root)?;
    ensure_private_directory(recovery_root)
}

fn valid_verification_id(id: &str) -> bool {
    id.strip_prefix("lvv-")
        .and_then(|value| Uuid::parse_str(value).ok())
        .is_some()
}

fn metadata_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_verification_id(id) {
        return Err("trusted Verify recovery id is malformed".into());
    }
    Ok(root.join(format!("{id}.json")))
}

fn visual_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_verification_id(id) {
        return Err("trusted Verify recovery id is malformed".into());
    }
    Ok(root.join(format!("{id}.png")))
}

fn consumed_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_verification_id(id) {
        return Err("trusted Verify recovery id is malformed".into());
    }
    Ok(root.join(format!("{id}.consumed")))
}

fn wave9_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_verification_id(id) {
        return Err("trusted Verify recovery id is malformed".into());
    }
    Ok(root.join(format!("{id}.wave9")))
}

fn write_new_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("create trusted Verify recovery file: {error}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("persist trusted Verify recovery file: {error}"));
    }
    drop(file);
    Ok(())
}

fn read_bounded_regular_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("inspect trusted Verify recovery file: {error}"))?;
    if metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || !metadata.is_file()
        || metadata.len() > max_bytes as u64
    {
        return Err("trusted Verify recovery file is unsafe or exceeds its bound".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| format!("open trusted Verify recovery file: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read trusted Verify recovery file: {error}"))?;
    if bytes.len() > max_bytes {
        return Err("trusted Verify recovery file exceeds its bound".into());
    }
    Ok(bytes)
}

fn persisted_record(record: &VerificationRecord) -> PersistedVerificationRecordV1 {
    let now_ms = now_unix_ms();
    let remaining_ms = record
        .expires_at
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    PersistedVerificationRecordV1 {
        schema_version: RECOVERY_SCHEMA_VERSION,
        verification_id: record.verification_id.clone(),
        proposal_id: record.proposal_id.clone(),
        session_id: record.session_id,
        reference: record.reference.clone(),
        canonical_route: record.canonical_route.clone(),
        canonical_file: record.canonical_file.clone(),
        project_root: record.project_root.clone(),
        display_file: record.display_file.clone(),
        source_line: record.source_line,
        postimage_sha256: record.postimage_sha256.clone(),
        instruction: record.instruction.clone(),
        semantic_before: record.semantic_before.clone(),
        visual_before: record.visual_before.as_ref().map(|visual| PersistedVisualBaselineV1 {
            png_sha256: sha256_bytes(visual.png.as_slice()),
            viewport: visual.viewport.clone(),
            pixel_width: visual.pixel_width,
            pixel_height: visual.pixel_height,
            target_rect: visual.target_rect.clone(),
            captured_at_unix_ms: visual.captured_at_unix_ms,
        }),
        scope: record.scope,
        created_at_unix_ms: now_ms,
        expires_at_unix_ms: now_ms.saturating_add(remaining_ms),
    }
}

fn persist_record(root: &Path, record: &VerificationRecord) -> Result<(), String> {
    ensure_private_directory(root)?;
    if record.status != VerificationStatus::Pending {
        return Err("trusted Verify can persist only pending recovery records".into());
    }

    let metadata = persisted_record(record);
    let metadata_bytes = serde_json::to_vec(&metadata)
        .map_err(|error| format!("serialize trusted Verify recovery metadata: {error}"))?;
    if metadata_bytes.len() > MAX_RECOVERY_METADATA_BYTES {
        return Err("trusted Verify recovery metadata exceeds its bound".into());
    }
    let wave9_bytes = record
        .wave9_preflight
        .as_ref()
        .map(|wave9_preflight| {
            serde_json::to_vec(&PersistedWave9PreflightV1 {
                schema_version: WAVE9_RECOVERY_SCHEMA_VERSION,
                verification_id: record.verification_id.clone(),
                wave9_preflight: wave9_preflight.clone(),
            })
            .map_err(|error| format!("serialize trusted Verify Wave 9 recovery context: {error}"))
        })
        .transpose()?;
    if wave9_bytes
        .as_ref()
        .is_some_and(|bytes| bytes.len() > MAX_RECOVERY_WAVE9_BYTES)
    {
        return Err("trusted Verify Wave 9 recovery context exceeds its bound".into());
    }

    let meta = metadata_path(root, &record.verification_id)?;
    let visual = visual_path(root, &record.verification_id)?;
    let wave9 = wave9_path(root, &record.verification_id)?;
    let consumed = consumed_path(root, &record.verification_id)?;
    if meta.exists() || visual.exists() || wave9.exists() || consumed.exists() {
        return Err("trusted Verify recovery record already exists".into());
    }

    if let Some(before) = record.visual_before.as_ref() {
        if before.png.len() > MAX_VERIFY_VISUAL_BYTES_PER_RECORD {
            return Err("trusted Verify recovery visual baseline exceeds its bound".into());
        }
        write_new_private_file(&visual, before.png.as_slice())?;
    }

    if let Some(wave9_bytes) = wave9_bytes.as_ref() {
        let wave9_temp = root.join(format!("{}.wave9.tmp", record.verification_id));
        if wave9_temp.exists() {
            let _ = fs::remove_file(&wave9_temp);
        }
        if let Err(error) = write_new_private_file(&wave9_temp, wave9_bytes).and_then(|_| {
            fs::rename(&wave9_temp, &wave9)
                .map_err(|error| format!("commit trusted Verify Wave 9 recovery context: {error}"))
        }) {
            let _ = fs::remove_file(&wave9_temp);
            let _ = fs::remove_file(&visual);
            return Err(error);
        }
    }

    let temp = root.join(format!("{}.json.tmp", record.verification_id));
    if temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    if let Err(error) = write_new_private_file(&temp, &metadata_bytes)
        .and_then(|_| {
            fs::rename(&temp, &meta)
                .map_err(|error| format!("commit trusted Verify recovery metadata: {error}"))
        })
    {
        let _ = fs::remove_file(&temp);
        let _ = fs::remove_file(&visual);
        let _ = fs::remove_file(&wave9);
        return Err(error);
    }
    Ok(())
}

fn consume_record(root: &Path, id: &str) -> Result<(), String> {
    let meta = metadata_path(root, id)?;
    let visual = visual_path(root, id)?;
    let wave9 = wave9_path(root, id)?;
    let consumed = consumed_path(root, id)?;
    match fs::rename(&meta, &consumed) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && consumed.exists() => {}
        Err(error) => {
            return Err(format!(
                "retire trusted Verify recovery metadata before consumption: {error}"
            ));
        }
    }
    let _ = fs::remove_file(&visual);
    let _ = fs::remove_file(&wave9);
    let _ = fs::remove_file(&consumed);
    Ok(())
}

fn decode_persisted_record(bytes: &[u8]) -> Result<PersistedVerificationRecordV2, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("parse trusted Verify recovery metadata: {error}"))?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "trusted Verify recovery metadata is missing schema version".to_string())?;
    match version {
        1 => serde_json::from_value::<PersistedVerificationRecordV1>(value)
            .map(PersistedVerificationRecordV2::from)
            .map_err(|error| format!("parse trusted Verify v1 recovery metadata: {error}")),
        2 => {
            let mut record = serde_json::from_value::<PersistedVerificationRecordV2>(value)
                .map_err(|error| format!("parse trusted Verify v2 recovery metadata: {error}"))?;
            record.schema_version = RECOVERY_SCHEMA_VERSION;
            Ok(record)
        },
        _ => Err(format!(
            "trusted Verify recovery schema version {version} is unsupported"
        )),
    }
}

fn load_wave9_preflight(
    root: &Path,
    id: &str,
) -> Result<Option<localview_verification::ProductionCandidatePreflightReceipt>, String> {
    let path = wave9_path(root, id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = read_bounded_regular_file(&path, MAX_RECOVERY_WAVE9_BYTES)?;
    let persisted: PersistedWave9PreflightV1 = serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse trusted Verify Wave 9 recovery context: {error}"))?;
    if persisted.schema_version != WAVE9_RECOVERY_SCHEMA_VERSION
        || persisted.verification_id != id
    {
        return Err("trusted Verify Wave 9 recovery context version or identity mismatch".into());
    }
    Ok(Some(persisted.wave9_preflight))
}

fn load_records(root: &Path) -> Result<HashMap<String, VerificationRecord>, String> {
    ensure_private_directory(root)?;
    let now_ms = now_unix_ms();
    let now = Instant::now();
    let mut records = HashMap::new();
    let mut retained_visual_bytes = 0usize;
    let mut active_ids = HashSet::new();

    let entries = fs::read_dir(root)
        .map_err(|error| format!("list trusted Verify recovery directory: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read trusted Verify recovery directory: {error}"))?;

    for entry in &entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err("trusted Verify recovery contains a non-UTF8 filename".into());
        };
        if name.ends_with(".consumed") || name.ends_with(".tmp") {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !valid_verification_id(id) {
            return Err("trusted Verify recovery contains an invalid metadata filename".into());
        }

        let bytes = read_bounded_regular_file(&entry.path(), MAX_RECOVERY_METADATA_BYTES)?;
        let mut persisted = decode_persisted_record(&bytes)?;
        let companion_wave9 = load_wave9_preflight(root, id)?;
        if persisted.wave9_preflight.is_some() && companion_wave9.is_some() {
            return Err("trusted Verify recovery contains ambiguous Wave 9 context".into());
        }
        if persisted.wave9_preflight.is_none() {
            persisted.wave9_preflight = companion_wave9;
        }
        if persisted.schema_version != RECOVERY_SCHEMA_VERSION
            || persisted.verification_id != id
            || persisted.semantic_before.context_version != VERIFY_CONTEXT_VERSION
            || persisted.created_at_unix_ms > persisted.expires_at_unix_ms
        {
            return Err("trusted Verify recovery metadata version or identity mismatch".into());
        }
        if persisted.expires_at_unix_ms <= now_ms {
            let _ = fs::remove_file(entry.path());
            let _ = fs::remove_file(visual_path(root, id)?);
            let _ = fs::remove_file(wave9_path(root, id)?);
            continue;
        }
        if records.len() >= MAX_VERIFICATION_RECORDS {
            return Err("trusted Verify recovery exceeds the retained record bound".into());
        }

        let visual_before = if let Some(visual) = persisted.visual_before {
            let path = visual_path(root, id)?;
            let png = read_bounded_regular_file(&path, MAX_VERIFY_VISUAL_BYTES_PER_RECORD)?;
            if sha256_bytes(&png) != visual.png_sha256 {
                return Err("trusted Verify recovery visual baseline digest mismatch".into());
            }
            retained_visual_bytes = retained_visual_bytes
                .checked_add(png.len())
                .ok_or_else(|| "trusted Verify recovery visual budget overflow".to_string())?;
            if retained_visual_bytes > MAX_VERIFY_TOTAL_VISUAL_BYTES {
                return Err("trusted Verify recovery total visual budget exceeded".into());
            }
            Some(VerifyVisualBaseline {
                png: Arc::new(png),
                viewport: visual.viewport,
                pixel_width: visual.pixel_width,
                pixel_height: visual.pixel_height,
                target_rect: visual.target_rect,
                captured_at_unix_ms: visual.captured_at_unix_ms,
            })
        } else {
            None
        };

        let remaining = persisted.expires_at_unix_ms.saturating_sub(now_ms);
        let record = VerificationRecord {
            verification_id: persisted.verification_id.clone(),
            proposal_id: persisted.proposal_id,
            session_id: persisted.session_id,
            reference: persisted.reference,
            canonical_route: persisted.canonical_route,
            canonical_file: persisted.canonical_file,
            project_root: persisted.project_root,
            display_file: persisted.display_file,
            source_line: persisted.source_line,
            postimage_sha256: persisted.postimage_sha256,
            instruction: persisted.instruction,
            semantic_before: persisted.semantic_before,
            visual_before,
            scope: persisted.scope,
            wave9_preflight: persisted.wave9_preflight,
            created_at: now,
            expires_at: now + Duration::from_millis(remaining),
            status: VerificationStatus::Pending,
        };
        active_ids.insert(record.verification_id.clone());
        records.insert(record.verification_id.clone(), record);
    }

    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(id) = name.strip_suffix(".png")
            && valid_verification_id(id)
            && !active_ids.contains(id)
        {
            let _ = fs::remove_file(entry.path());
        }
        if let Some(id) = name.strip_suffix(".wave9")
            && valid_verification_id(id)
            && !active_ids.contains(id)
        {
            let _ = fs::remove_file(entry.path());
        }
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_recovery_record_migrates_without_wave9_authority() {
        let persisted = PersistedVerificationRecordV1 {
            schema_version: 1,
            verification_id: format!("lvv-{}", Uuid::new_v4()),
            proposal_id: format!("lvp-{}", Uuid::new_v4()),
            session_id: Uuid::new_v4(),
            reference: "@e1".into(),
            canonical_route: "http://127.0.0.1:5173/".into(),
            canonical_file: PathBuf::from("src/App.tsx"),
            project_root: PathBuf::from("."),
            display_file: "src/App.tsx".into(),
            source_line: 1,
            postimage_sha256: "sha256:fixture".into(),
            instruction: "fixture".into(),
            semantic_before: VerifySemanticBaseline {
                context_version: VERIFY_CONTEXT_VERSION,
                snapshot_version: 1,
                selected: crate::trusted_verify::VerifySemanticProjection {
                    reference: "@e1".into(),
                    role: Some("button".into()),
                    name: Some("Save".into()),
                    tag: "button".into(),
                    interactive: true,
                    attributes: Default::default(),
                    source: Some("src/App.tsx".into()),
                    rect: None,
                },
                console_issues: Vec::new(),
                network_issues: Vec::new(),
            },
            visual_before: None,
            scope: VerificationScope::SemanticOnly,
            created_at_unix_ms: 10,
            expires_at_unix_ms: 20,
        };
        let bytes = serde_json::to_vec(&persisted).expect("serialize v1 recovery fixture");
        assert!(
            !String::from_utf8(bytes.clone())
                .expect("v1 recovery fixture is utf8")
                .contains("wave9_preflight"),
            "rollback-readable primary metadata must not gain Wave 9-only fields"
        );
        let migrated = decode_persisted_record(&bytes).expect("migrate v1 recovery fixture");
        assert_eq!(migrated.schema_version, RECOVERY_SCHEMA_VERSION);
        assert_eq!(migrated.verification_id, persisted.verification_id);
        assert!(migrated.wave9_preflight.is_none());
    }

    #[test]
    fn wave9_companion_is_versioned_separately_from_rollback_readable_metadata() {
        let verification_id = format!("lvv-{}", Uuid::new_v4());
        let persisted = PersistedWave9PreflightV1 {
            schema_version: WAVE9_RECOVERY_SCHEMA_VERSION,
            verification_id: verification_id.clone(),
            wave9_preflight:
                localview_verification::ProductionCandidatePreflightReceipt::inconclusive_unavailable(
                    "fixture",
                ),
        };
        let bytes = serde_json::to_vec(&persisted).expect("serialize Wave 9 companion");
        let decoded: PersistedWave9PreflightV1 =
            serde_json::from_slice(&bytes).expect("decode Wave 9 companion");
        assert_eq!(decoded.schema_version, WAVE9_RECOVERY_SCHEMA_VERSION);
        assert_eq!(decoded.verification_id, verification_id);
        assert_eq!(RECOVERY_SCHEMA_VERSION, 1);
    }

    #[test]
    fn verification_id_is_path_safe_uuid_only() {
        assert!(valid_verification_id(&format!("lvv-{}", Uuid::new_v4())));
        for invalid in ["", "lvv-", "../escape", "lvv-../../escape", "other-123"] {
            assert!(!valid_verification_id(invalid), "{invalid}");
        }
    }
}
