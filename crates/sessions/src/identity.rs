use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use atomic_write_file::AtomicWriteFile;
use localview_protocol::{Endpoint, ProjectIdentity, ServerKind, SessionId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const SESSION_IDENTITY_REGISTRY_FILE: &str = "session-identities-v1.json";
pub const MAX_SESSION_IDENTITY_REGISTRY_BYTES: usize = 1_048_576;
pub const MAX_SESSION_IDENTITY_RECORDS: usize = 4_096;
pub const MAX_NORMALIZED_PROJECT_PATH_BYTES: usize = 4_096;
pub const MAX_ENDPOINT_HOST_BYTES: usize = 255;
pub const MAX_ENDPOINT_SCHEME_BYTES: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(tag = "lineage_version", content = "value")]
pub enum SessionLineage {
    #[serde(rename = "localview_session_lineage_v1")]
    V1(SessionLineageV1),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionLineageV1 {
    pub anchor: SessionLineageAnchorV1,
    pub server_kind: SessionServerKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(tag = "anchor_type", rename_all = "snake_case")]
pub enum SessionLineageAnchorV1 {
    Project {
        normalized_project_path: String,
    },
    Endpoint {
        scheme: String,
        host: String,
        port: u16,
    },
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum SessionServerKind {
    FrontendDevServer,
    Storybook,
    ApiServer,
    StaticSite,
    UnknownHttp,
}

impl From<ServerKind> for SessionServerKind {
    fn from(value: ServerKind) -> Self {
        match value {
            ServerKind::FrontendDevServer => Self::FrontendDevServer,
            ServerKind::Storybook => Self::Storybook,
            ServerKind::ApiServer => Self::ApiServer,
            ServerKind::StaticSite => Self::StaticSite,
            ServerKind::UnknownHttp => Self::UnknownHttp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SessionIdentityError {
    #[error("project path is empty or exceeds the durable identity limit")]
    InvalidProjectPath,
    #[error("project path contains unresolved parent traversal")]
    ParentTraversal,
    #[error("endpoint scheme is invalid for durable identity")]
    InvalidEndpointScheme,
    #[error("endpoint host is invalid for durable identity")]
    InvalidEndpointHost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdentityHealth {
    Healthy,
    VolatileDegraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdentityDurability {
    Durable,
    Volatile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedSessionIdentity {
    pub session_id: SessionId,
    pub durability: SessionIdentityDurability,
}

#[derive(Debug, Clone)]
pub struct SessionIdentityResolver {
    inner: Arc<Mutex<SessionIdentityResolverState>>,
    registry_path: PathBuf,
    commit_gate: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Debug)]
struct SessionIdentityResolverState {
    health: SessionIdentityHealth,
    diagnostic: Option<String>,
    records: BTreeMap<SessionLineage, SessionId>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionIdentityRegistryFile {
    schema_version: u32,
    records: Vec<SessionIdentityRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionIdentityRecord {
    lineage: SessionLineage,
    session_id: SessionId,
}

#[derive(Debug, Error)]
enum RegistryLoadError {
    #[error("session identity registry I/O failed: {0}")]
    Io(String),
    #[error("session identity registry exceeds {MAX_SESSION_IDENTITY_REGISTRY_BYTES} bytes")]
    TooLarge,
    #[error("session identity registry is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("session identity registry schema version is missing or invalid")]
    InvalidSchemaVersion,
    #[error("session identity registry schema version {0} is unsupported")]
    UnsupportedSchemaVersion(u64),
    #[error("session identity registry contains too many records")]
    TooManyRecords,
    #[error("session identity registry contains a noncanonical lineage")]
    NonCanonicalLineage,
    #[error("session identity registry contains a nil session UUID")]
    NilSessionId,
    #[error("session identity registry maps one lineage more than once")]
    DuplicateLineage,
    #[error("session identity registry maps one session UUID to multiple lineages")]
    DuplicateSessionId,
}

#[derive(Debug, Error)]
enum RegistryCommitError {
    #[error("session identity registry serialization failed: {0}")]
    Serialization(String),
    #[error("session identity registry commit would exceed {MAX_SESSION_IDENTITY_REGISTRY_BYTES} bytes")]
    TooLarge,
    #[error("session identity registry commit I/O failed: {0}")]
    Io(String),
}

impl SessionIdentityResolver {
    pub async fn open_file(path: PathBuf) -> Self {
        let load_path = path.clone();
        let loaded = tokio::task::spawn_blocking(move || load_registry(&load_path)).await;
        match loaded {
            Ok(Ok(records)) => Self {
                inner: Arc::new(Mutex::new(SessionIdentityResolverState {
                    health: SessionIdentityHealth::Healthy,
                    diagnostic: None,
                    records,
                })),
                registry_path: path,
                commit_gate: Arc::new(tokio::sync::Mutex::new(())),
            },
            Ok(Err(error)) => Self::degraded(path, error.to_string()),
            Err(error) => Self::degraded(
                path,
                format!("session identity registry loader task failed: {error}"),
            ),
        }
    }

    pub fn health(&self) -> SessionIdentityHealth {
        self.lock().health
    }

    pub fn diagnostic(&self) -> Option<String> {
        self.lock().diagnostic.clone()
    }

    pub fn record_count(&self) -> usize {
        self.lock().records.len()
    }

    pub async fn existing(&self, lineage: &SessionLineage) -> Option<SessionId> {
        let _commit_guard = self.commit_gate.lock().await;
        let state = self.lock();
        if state.health != SessionIdentityHealth::Healthy {
            return None;
        }
        state.records.get(lineage).copied()
    }

    pub async fn resolve_new(&self, lineage: &SessionLineage) -> ResolvedSessionIdentity {
        let _commit_guard = self.commit_gate.lock().await;

        let (candidate, next_records) = {
            let state = self.lock();
            if state.health != SessionIdentityHealth::Healthy {
                return volatile_identity(&state.records);
            }
            if let Some(session_id) = state.records.get(lineage).copied() {
                return ResolvedSessionIdentity {
                    session_id,
                    durability: SessionIdentityDurability::Durable,
                };
            }
            if state.records.len() >= MAX_SESSION_IDENTITY_RECORDS
                || validate_canonical_lineage(lineage).is_err()
            {
                return volatile_identity(&state.records);
            }

            let candidate = fresh_session_id(&state.records);
            let mut next_records = state.records.clone();
            next_records.insert(lineage.clone(), candidate);
            (candidate, next_records)
        };

        let path = self.registry_path.clone();
        let records_for_commit = next_records.clone();
        let committed = tokio::task::spawn_blocking(move || {
            commit_registry(&path, &records_for_commit)
        })
        .await;

        match committed {
            Ok(Ok(())) => {
                let mut state = self.lock();
                state.records = next_records;
                ResolvedSessionIdentity {
                    session_id: candidate,
                    durability: SessionIdentityDurability::Durable,
                }
            }
            Ok(Err(error)) => {
                self.mark_degraded(error.to_string());
                let state = self.lock();
                volatile_identity(&state.records)
            }
            Err(error) => {
                self.mark_degraded(format!(
                    "session identity registry commit task failed: {error}"
                ));
                let state = self.lock();
                volatile_identity(&state.records)
            }
        }
    }

    fn degraded(path: PathBuf, diagnostic: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(SessionIdentityResolverState {
                health: SessionIdentityHealth::VolatileDegraded,
                diagnostic: Some(diagnostic),
                records: BTreeMap::new(),
            })),
            registry_path: path,
            commit_gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn mark_degraded(&self, diagnostic: String) {
        let mut state = self.lock();
        state.health = SessionIdentityHealth::VolatileDegraded;
        state.diagnostic = Some(diagnostic);
    }

    fn lock(&self) -> MutexGuard<'_, SessionIdentityResolverState> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn session_lineage(
    project: &ProjectIdentity,
    endpoint: &Endpoint,
    kind: ServerKind,
) -> Result<SessionLineage, SessionIdentityError> {
    let anchor = match project.git_root.as_deref().or(project.cwd.as_deref()) {
        Some(path) if !path.is_empty() => SessionLineageAnchorV1::Project {
            normalized_project_path: normalize_project_path(path)?,
        },
        _ => SessionLineageAnchorV1::Endpoint {
            scheme: validate_endpoint_part(
                &endpoint.scheme,
                MAX_ENDPOINT_SCHEME_BYTES,
                SessionIdentityError::InvalidEndpointScheme,
            )?,
            host: validate_endpoint_part(
                &endpoint.host,
                MAX_ENDPOINT_HOST_BYTES,
                SessionIdentityError::InvalidEndpointHost,
            )?,
            port: endpoint.port,
        },
    };

    Ok(SessionLineage::V1(SessionLineageV1 {
        anchor,
        server_kind: kind.into(),
    }))
}

fn load_registry(
    path: &Path,
) -> Result<BTreeMap<SessionLineage, SessionId>, RegistryLoadError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BTreeMap::new());
        }
        Err(error) => return Err(RegistryLoadError::Io(error.to_string())),
    };

    let mut bytes = Vec::new();
    file.take((MAX_SESSION_IDENTITY_REGISTRY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| RegistryLoadError::Io(error.to_string()))?;
    if bytes.len() > MAX_SESSION_IDENTITY_REGISTRY_BYTES {
        return Err(RegistryLoadError::TooLarge);
    }

    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| RegistryLoadError::InvalidJson(error.to_string()))?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(RegistryLoadError::InvalidSchemaVersion)?;
    if version != 1 {
        return Err(RegistryLoadError::UnsupportedSchemaVersion(version));
    }
    let record_count = value
        .get("records")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| RegistryLoadError::InvalidJson("records must be an array".into()))?;
    if record_count > MAX_SESSION_IDENTITY_RECORDS {
        return Err(RegistryLoadError::TooManyRecords);
    }

    let registry: SessionIdentityRegistryFile = serde_json::from_value(value)
        .map_err(|error| RegistryLoadError::InvalidJson(error.to_string()))?;
    if registry.schema_version != 1 {
        return Err(RegistryLoadError::UnsupportedSchemaVersion(
            u64::from(registry.schema_version),
        ));
    }

    validate_records(registry.records)
}

fn commit_registry(
    path: &Path,
    records: &BTreeMap<SessionLineage, SessionId>,
) -> Result<(), RegistryCommitError> {
    let registry = SessionIdentityRegistryFile {
        schema_version: 1,
        records: records
            .iter()
            .map(|(lineage, session_id)| SessionIdentityRecord {
                lineage: lineage.clone(),
                session_id: *session_id,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&registry)
        .map_err(|error| RegistryCommitError::Serialization(error.to_string()))?;
    if bytes.len() > MAX_SESSION_IDENTITY_REGISTRY_BYTES {
        return Err(RegistryCommitError::TooLarge);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| RegistryCommitError::Io(error.to_string()))?;
    }
    let mut file = AtomicWriteFile::open(path)
        .map_err(|error| RegistryCommitError::Io(error.to_string()))?;
    file.write_all(&bytes)
        .map_err(|error| RegistryCommitError::Io(error.to_string()))?;
    file.sync_all()
        .map_err(|error| RegistryCommitError::Io(error.to_string()))?;
    file.commit()
        .map_err(|error| RegistryCommitError::Io(error.to_string()))
}

fn validate_records(
    records: Vec<SessionIdentityRecord>,
) -> Result<BTreeMap<SessionLineage, SessionId>, RegistryLoadError> {
    if records.len() > MAX_SESSION_IDENTITY_RECORDS {
        return Err(RegistryLoadError::TooManyRecords);
    }

    let mut by_lineage = BTreeMap::new();
    let mut session_ids = HashSet::new();
    for record in records {
        validate_canonical_lineage(&record.lineage)?;
        if record.session_id == Uuid::nil() {
            return Err(RegistryLoadError::NilSessionId);
        }
        if !session_ids.insert(record.session_id) {
            return Err(RegistryLoadError::DuplicateSessionId);
        }
        if by_lineage
            .insert(record.lineage, record.session_id)
            .is_some()
        {
            return Err(RegistryLoadError::DuplicateLineage);
        }
    }
    Ok(by_lineage)
}

fn validate_canonical_lineage(lineage: &SessionLineage) -> Result<(), RegistryLoadError> {
    match lineage {
        SessionLineage::V1(value) => match &value.anchor {
            SessionLineageAnchorV1::Project {
                normalized_project_path,
            } => {
                let normalized = normalize_project_path(normalized_project_path)
                    .map_err(|_| RegistryLoadError::NonCanonicalLineage)?;
                if normalized != *normalized_project_path {
                    return Err(RegistryLoadError::NonCanonicalLineage);
                }
            }
            SessionLineageAnchorV1::Endpoint {
                scheme,
                host,
                port: _,
            } => {
                validate_endpoint_part(
                    scheme,
                    MAX_ENDPOINT_SCHEME_BYTES,
                    SessionIdentityError::InvalidEndpointScheme,
                )
                .map_err(|_| RegistryLoadError::NonCanonicalLineage)?;
                validate_endpoint_part(
                    host,
                    MAX_ENDPOINT_HOST_BYTES,
                    SessionIdentityError::InvalidEndpointHost,
                )
                .map_err(|_| RegistryLoadError::NonCanonicalLineage)?;
            }
        },
    }
    Ok(())
}

fn fresh_session_id(records: &BTreeMap<SessionLineage, SessionId>) -> SessionId {
    loop {
        let candidate = Uuid::new_v4();
        if candidate != Uuid::nil() && !records.values().any(|session_id| *session_id == candidate) {
            return candidate;
        }
    }
}

fn volatile_identity(records: &BTreeMap<SessionLineage, SessionId>) -> ResolvedSessionIdentity {
    ResolvedSessionIdentity {
        session_id: fresh_session_id(records),
        durability: SessionIdentityDurability::Volatile,
    }
}

fn validate_endpoint_part(
    value: &str,
    max_bytes: usize,
    error: SessionIdentityError,
) -> Result<String, SessionIdentityError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(error);
    }
    Ok(value.to_owned())
}

fn normalize_project_path(raw: &str) -> Result<String, SessionIdentityError> {
    normalize_project_path_for_flavor(raw, PathFlavor::current())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathFlavor {
    Unix,
    Windows,
}

impl PathFlavor {
    const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

fn normalize_project_path_for_flavor(
    raw: &str,
    flavor: PathFlavor,
) -> Result<String, SessionIdentityError> {
    if raw.is_empty()
        || raw.len() > MAX_NORMALIZED_PROJECT_PATH_BYTES
        || raw.chars().any(char::is_control)
    {
        return Err(SessionIdentityError::InvalidProjectPath);
    }

    match flavor {
        PathFlavor::Unix => normalize_unix_path(raw),
        PathFlavor::Windows => normalize_windows_path(raw),
    }
}

fn normalized_components<'a>(
    components: impl Iterator<Item = &'a str>,
) -> Result<Vec<&'a str>, SessionIdentityError> {
    let mut normalized = Vec::new();
    for component in components {
        match component {
            "" | "." => {}
            ".." => return Err(SessionIdentityError::ParentTraversal),
            value => normalized.push(value),
        }
    }
    Ok(normalized)
}

fn normalize_unix_path(raw: &str) -> Result<String, SessionIdentityError> {
    let rooted = raw.starts_with('/');
    let components = normalized_components(raw.split('/'))?;
    if components.is_empty() {
        return if rooted {
            Ok("/".into())
        } else {
            Err(SessionIdentityError::InvalidProjectPath)
        };
    }

    let joined = components.join("/");
    Ok(if rooted {
        format!("/{joined}")
    } else {
        joined
    })
}

fn normalize_windows_path(raw: &str) -> Result<String, SessionIdentityError> {
    let replaced = raw.replace('\\', "/").to_lowercase();
    let bytes = replaced.as_bytes();
    let is_unc = replaced.starts_with("//");
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';

    let (prefix, rest) = if is_unc {
        ("//".to_owned(), replaced.trim_start_matches('/'))
    } else if has_drive {
        let drive = &replaced[..2];
        let after_drive = &replaced[2..];
        if after_drive.starts_with('/') {
            (format!("{drive}/"), after_drive.trim_start_matches('/'))
        } else {
            (drive.to_owned(), after_drive)
        }
    } else if replaced.starts_with('/') {
        ("/".to_owned(), replaced.trim_start_matches('/'))
    } else {
        (String::new(), replaced.as_str())
    };

    let components = normalized_components(rest.split('/'))?;
    if components.is_empty() {
        return if prefix.is_empty() {
            Err(SessionIdentityError::InvalidProjectPath)
        } else {
            Ok(prefix)
        };
    }

    Ok(format!("{prefix}{}", components.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_normalization_is_lexical_and_case_preserving() {
        assert_eq!(
            normalize_project_path_for_flavor("/Work/./app//", PathFlavor::Unix).unwrap(),
            "/Work/app"
        );
        assert_ne!(
            normalize_project_path_for_flavor("/Work/app", PathFlavor::Unix).unwrap(),
            normalize_project_path_for_flavor("/work/app", PathFlavor::Unix).unwrap()
        );
    }

    #[test]
    fn windows_normalization_is_separator_and_case_stable() {
        let backslash = normalize_project_path_for_flavor(
            r"C:\\Users\\Dev\\App\\",
            PathFlavor::Windows,
        )
        .unwrap();
        let slash = normalize_project_path_for_flavor("c:/users/dev/app", PathFlavor::Windows)
            .unwrap();
        assert_eq!(backslash, "c:/users/dev/app");
        assert_eq!(backslash, slash);
    }

    #[test]
    fn unresolved_parent_is_rejected_for_both_flavors() {
        assert_eq!(
            normalize_project_path_for_flavor("/work/../app", PathFlavor::Unix),
            Err(SessionIdentityError::ParentTraversal)
        );
        assert_eq!(
            normalize_project_path_for_flavor(r"C:\\work\\..\\app", PathFlavor::Windows),
            Err(SessionIdentityError::ParentTraversal)
        );
    }
}
