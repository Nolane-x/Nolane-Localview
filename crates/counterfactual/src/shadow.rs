use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{CounterfactualCandidate, IsolationLevel, SourceOverlay};

pub const MAX_SHADOW_FILES: usize = 16;
pub const MAX_SHADOW_PATCH_BYTES: usize = 512 * 1024;
pub const MAX_SHADOW_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SHADOW_STARTUP_MS: u64 = 20_000;
pub const MAX_SHADOW_LIFETIME_MS: u64 = 120_000;

const SENSITIVE_BASENAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.development",
    ".env.production",
    ".env.test",
    "credentials",
    "credentials.json",
    "secrets.json",
    "service-account.json",
    "id_rsa",
    "id_ed25519",
    ".npmrc",
    ".pypirc",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShadowCandidateProof {
    pub candidate_id: Uuid,
    pub base_revision: String,
    pub patch_digest: String,
    pub isolation: IsolationLevel,
    pub changed_files: Vec<String>,
    pub shadow_path: String,
    pub original_worktree_dirty: bool,
    pub real_worktree_unchanged: bool,
    pub external_side_effects_blocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShadowCleanupProof {
    pub attempted: bool,
    pub worktree_removed: bool,
    pub directory_absent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ShadowError {
    NonGitProject,
    GitUnavailable,
    BaseRevisionMismatch { expected: String, actual: String },
    BaseRevisionNotExact,
    NotDisposable,
    UnsupportedIsolation,
    EmptyOverlaySet,
    TooManyFiles,
    OversizedPatch,
    InvalidPath { path: String },
    SecretFile { path: String },
    SensitiveTrackedFile { path: String },
    SymlinkEscape { path: String },
    UnsupportedBinaryFile { path: String },
    OversizedSourceFile { path: String },
    MissingSourceFile { path: String },
    FileHashMismatch { path: String },
    PatchTouchesUnexpectedPath { expected: String, actual: String },
    PatchRejected { path: String },
    ShadowWorktreeCreateFailed,
    ShadowCheckoutFailed,
    ShadowDiffFailed,
    UnexpectedChangedFile { path: String },
    CleanupFailed,
}

#[derive(Debug)]
pub struct ShadowWorkspace {
    repository_root: PathBuf,
    shadow_root: PathBuf,
    candidate_id: Uuid,
    base_revision: String,
    patch_digest: String,
    isolation: IsolationLevel,
    changed_files: Vec<String>,
    original_status: String,
    cleaned: bool,
}

impl ShadowWorkspace {
    pub fn prepare(
        repository_root: &Path,
        candidate: &CounterfactualCandidate,
    ) -> Result<Self, ShadowError> {
        candidate.validate().map_err(|_| ShadowError::NotDisposable)?;
        if !candidate.disposable {
            return Err(ShadowError::NotDisposable);
        }
        if !matches!(
            candidate.isolation,
            IsolationLevel::NativeWebView | IsolationLevel::ChromiumSandbox | IsolationLevel::SemanticOnly
        ) {
            return Err(ShadowError::UnsupportedIsolation);
        }
        if candidate.overlays.is_empty() {
            return Err(ShadowError::EmptyOverlaySet);
        }
        if candidate.overlays.len() > MAX_SHADOW_FILES {
            return Err(ShadowError::TooManyFiles);
        }
        let total_patch_bytes = candidate
            .overlays
            .iter()
            .try_fold(0usize, |total, overlay| total.checked_add(overlay.patch.len()))
            .ok_or(ShadowError::OversizedPatch)?;
        if total_patch_bytes > MAX_SHADOW_PATCH_BYTES {
            return Err(ShadowError::OversizedPatch);
        }

        let repository_root = fs::canonicalize(repository_root).map_err(|_| ShadowError::NonGitProject)?;
        let top = git_output(&repository_root, &["rev-parse", "--show-toplevel"])
            .ok_or(ShadowError::NonGitProject)?;
        let canonical_top = fs::canonicalize(top.trim()).map_err(|_| ShadowError::NonGitProject)?;
        if canonical_top != repository_root {
            return Err(ShadowError::NonGitProject);
        }
        let head = git_output(&repository_root, &["rev-parse", "HEAD"])
            .ok_or(ShadowError::GitUnavailable)?
            .trim()
            .to_owned();
        if head != candidate.base_revision {
            return Err(ShadowError::BaseRevisionMismatch {
                expected: candidate.base_revision.clone(),
                actual: head,
            });
        }
        let resolved = git_output(
            &repository_root,
            &["rev-parse", &format!("{}^{{commit}}", candidate.base_revision)],
        )
        .ok_or(ShadowError::GitUnavailable)?
        .trim()
        .to_owned();
        if resolved != candidate.base_revision || !is_exact_object_id(&candidate.base_revision) {
            return Err(ShadowError::BaseRevisionNotExact);
        }

        reject_tracked_secrets(&repository_root, &candidate.base_revision)?;

        let mut changed_files = BTreeSet::new();
        for overlay in &candidate.overlays {
            validate_overlay(&repository_root, &candidate.base_revision, overlay)?;
            changed_files.insert(overlay.file.clone());
        }

        let original_status = git_output(&repository_root, &["status", "--porcelain=v1"])
            .ok_or(ShadowError::GitUnavailable)?;

        let shadow_root = std::env::temp_dir().join(format!(
            "localview-wave9-shadow-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        let status = Command::new("git")
            .arg("-C")
            .arg(&repository_root)
            .args(["worktree", "add", "--detach", "--no-checkout"])
            .arg(&shadow_root)
            .arg(&candidate.base_revision)
            .status()
            .map_err(|_| ShadowError::GitUnavailable)?;
        if !status.success() {
            return Err(ShadowError::ShadowWorktreeCreateFailed);
        }

        let mut workspace = Self {
            repository_root,
            shadow_root,
            candidate_id: candidate.id,
            base_revision: candidate.base_revision.clone(),
            patch_digest: patch_digest(&candidate.overlays),
            isolation: candidate.isolation,
            changed_files: changed_files.into_iter().collect(),
            original_status,
            cleaned: false,
        };

        let checkout = Command::new("git")
            .arg("-C")
            .arg(&workspace.shadow_root)
            .args(["checkout", "--force", &workspace.base_revision, "--", "."])
            .status()
            .map_err(|_| ShadowError::GitUnavailable)?;
        if !checkout.success() {
            let _ = workspace.cleanup();
            return Err(ShadowError::ShadowCheckoutFailed);
        }

        for overlay in &candidate.overlays {
            if let Err(error) = apply_overlay(&workspace.shadow_root, overlay) {
                let _ = workspace.cleanup();
                return Err(error);
            }
        }

        let actual = git_output(&workspace.shadow_root, &["diff", "--name-only", "--"])
            .ok_or(ShadowError::ShadowDiffFailed)?;
        let actual_files = actual
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let expected = workspace.changed_files.iter().cloned().collect::<BTreeSet<_>>();
        if let Some(path) = actual_files.difference(&expected).next() {
            let error = ShadowError::UnexpectedChangedFile { path: path.clone() };
            let _ = workspace.cleanup();
            return Err(error);
        }

        Ok(workspace)
    }

    pub fn root(&self) -> &Path {
        &self.shadow_root
    }

    pub fn proof(&self) -> Result<ShadowCandidateProof, ShadowError> {
        let current_status = git_output(&self.repository_root, &["status", "--porcelain=v1"])
            .ok_or(ShadowError::GitUnavailable)?;
        Ok(ShadowCandidateProof {
            candidate_id: self.candidate_id,
            base_revision: self.base_revision.clone(),
            patch_digest: self.patch_digest.clone(),
            isolation: self.isolation,
            changed_files: self.changed_files.clone(),
            shadow_path: self.shadow_root.to_string_lossy().into_owned(),
            original_worktree_dirty: !self.original_status.trim().is_empty(),
            real_worktree_unchanged: current_status == self.original_status,
            external_side_effects_blocked: true,
        })
    }

    pub fn cleanup(&mut self) -> Result<ShadowCleanupProof, ShadowError> {
        if self.cleaned {
            return Ok(ShadowCleanupProof {
                attempted: true,
                worktree_removed: true,
                directory_absent: !self.shadow_root.exists(),
            });
        }
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.repository_root)
            .args(["worktree", "remove", "--force"])
            .arg(&self.shadow_root)
            .status()
            .map_err(|_| ShadowError::GitUnavailable)?;
        let directory_absent = !self.shadow_root.exists();
        self.cleaned = status.success() && directory_absent;
        let proof = ShadowCleanupProof {
            attempted: true,
            worktree_removed: status.success(),
            directory_absent,
        };
        if self.cleaned {
            Ok(proof)
        } else {
            Err(ShadowError::CleanupFailed)
        }
    }
}

impl Drop for ShadowWorkspace {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.cleanup();
        }
    }
}

#[derive(Debug)]
pub struct LoopbackPortReservation {
    listener: TcpListener,
}

impl LoopbackPortReservation {
    pub fn reserve() -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
        Ok(Self { listener })
    }

    pub fn port(&self) -> Result<u16, std::io::Error> {
        Ok(self.listener.local_addr()?.port())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShadowLaunchPolicy {
    pub loopback_only: bool,
    pub network_isolation_proven: bool,
    pub production_service_isolation_proven: bool,
    pub startup_timeout_ms: u64,
    pub lifetime_ms: u64,
}

impl Default for ShadowLaunchPolicy {
    fn default() -> Self {
        Self {
            loopback_only: true,
            network_isolation_proven: false,
            production_service_isolation_proven: false,
            startup_timeout_ms: MAX_SHADOW_STARTUP_MS,
            lifetime_ms: MAX_SHADOW_LIFETIME_MS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ShadowLaunchBlocker {
    NonLoopbackRequested,
    NetworkIsolationUnproven,
    ProductionServiceIsolationUnproven,
    InvalidStartupTimeout,
    InvalidLifetime,
}

pub fn authorize_shadow_launch(
    bind_host: &str,
    policy: &ShadowLaunchPolicy,
) -> Result<(), ShadowLaunchBlocker> {
    let is_loopback = matches!(bind_host, "127.0.0.1" | "::1" | "localhost");
    if !policy.loopback_only || !is_loopback {
        return Err(ShadowLaunchBlocker::NonLoopbackRequested);
    }
    if !policy.network_isolation_proven {
        return Err(ShadowLaunchBlocker::NetworkIsolationUnproven);
    }
    if !policy.production_service_isolation_proven {
        return Err(ShadowLaunchBlocker::ProductionServiceIsolationUnproven);
    }
    if policy.startup_timeout_ms == 0 || policy.startup_timeout_ms > MAX_SHADOW_STARTUP_MS {
        return Err(ShadowLaunchBlocker::InvalidStartupTimeout);
    }
    if policy.lifetime_ms == 0 || policy.lifetime_ms > MAX_SHADOW_LIFETIME_MS {
        return Err(ShadowLaunchBlocker::InvalidLifetime);
    }
    Ok(())
}

pub fn patch_digest(overlays: &[SourceOverlay]) -> String {
    let mut hasher = Sha256::new();
    for overlay in overlays {
        hasher.update(overlay.file.as_bytes());
        hasher.update([0]);
        hasher.update(overlay.base_hash.as_bytes());
        hasher.update([0]);
        hasher.update(overlay.patch.as_bytes());
        hasher.update([0xff]);
    }
    format!("sha256:{}", hex_lower(&hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_lower(&Sha256::digest(bytes)))
}

fn validate_overlay(
    repository_root: &Path,
    base_revision: &str,
    overlay: &SourceOverlay,
) -> Result<(), ShadowError> {
    validate_relative_path(&overlay.file)?;
    if is_sensitive_path(&overlay.file) {
        return Err(ShadowError::SecretFile {
            path: overlay.file.clone(),
        });
    }
    validate_patch_paths(overlay)?;

    let tree_entry = git_output(
        repository_root,
        &["ls-tree", base_revision, "--", &overlay.file],
    )
    .ok_or(ShadowError::GitUnavailable)?;
    if tree_entry.trim().is_empty() {
        return Err(ShadowError::MissingSourceFile {
            path: overlay.file.clone(),
        });
    }
    if tree_entry.starts_with("120000 ") {
        return Err(ShadowError::SymlinkEscape {
            path: overlay.file.clone(),
        });
    }

    let bytes = git_bytes(
        repository_root,
        &["show", &format!("{base_revision}:{}", overlay.file)],
    )
    .ok_or_else(|| ShadowError::MissingSourceFile {
        path: overlay.file.clone(),
    })?;
    if bytes.len() > MAX_SHADOW_FILE_BYTES {
        return Err(ShadowError::OversizedSourceFile {
            path: overlay.file.clone(),
        });
    }
    if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        return Err(ShadowError::UnsupportedBinaryFile {
            path: overlay.file.clone(),
        });
    }
    let actual_hash = sha256_bytes(&bytes);
    if actual_hash != overlay.base_hash {
        return Err(ShadowError::FileHashMismatch {
            path: overlay.file.clone(),
        });
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), ShadowError> {
    if path.is_empty() {
        return Err(ShadowError::InvalidPath { path: path.into() });
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() || parsed.has_root() {
        return Err(ShadowError::InvalidPath { path: path.into() });
    }
    for component in parsed.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ShadowError::InvalidPath { path: path.into() });
            }
        }
    }
    Ok(())
}

fn is_sensitive_path(path: &str) -> bool {
    let parsed = Path::new(path);
    parsed.components().any(|component| {
        let Component::Normal(value) = component else {
            return false;
        };
        let lower = value.to_string_lossy().to_ascii_lowercase();
        lower == ".git"
            || lower == ".ssh"
            || lower == ".aws"
            || lower == ".gnupg"
            || SENSITIVE_BASENAMES
                .iter()
                .any(|candidate| lower == *candidate || lower.starts_with(".env."))
    })
}

fn reject_tracked_secrets(repository_root: &Path, base_revision: &str) -> Result<(), ShadowError> {
    let tracked = git_output(
        repository_root,
        &["ls-tree", "-r", "--name-only", base_revision],
    )
    .ok_or(ShadowError::GitUnavailable)?;
    if let Some(path) = tracked.lines().find(|path| is_sensitive_path(path)) {
        return Err(ShadowError::SensitiveTrackedFile {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn validate_patch_paths(overlay: &SourceOverlay) -> Result<(), ShadowError> {
    if overlay.patch.contains("GIT binary patch") || overlay.patch.contains("Binary files ") {
        return Err(ShadowError::UnsupportedBinaryFile {
            path: overlay.file.clone(),
        });
    }
    for line in overlay.patch.lines() {
        let Some(raw) = line.strip_prefix("--- ").or_else(|| line.strip_prefix("+++ ")) else {
            continue;
        };
        let raw = raw.split_whitespace().next().unwrap_or(raw);
        if raw == "/dev/null" {
            return Err(ShadowError::PatchTouchesUnexpectedPath {
                expected: overlay.file.clone(),
                actual: raw.into(),
            });
        }
        let normalized = raw
            .strip_prefix("a/")
            .or_else(|| raw.strip_prefix("b/"))
            .unwrap_or(raw);
        if normalized != overlay.file {
            return Err(ShadowError::PatchTouchesUnexpectedPath {
                expected: overlay.file.clone(),
                actual: normalized.into(),
            });
        }
    }
    Ok(())
}

fn apply_overlay(shadow_root: &Path, overlay: &SourceOverlay) -> Result<(), ShadowError> {
    for check_only in [true, false] {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(shadow_root)
            .args(["apply", "--whitespace=nowarn"]);
        if check_only {
            command.arg("--check");
        }
        command
            .arg("--")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| ShadowError::GitUnavailable)?;
        child
            .stdin
            .take()
            .ok_or_else(|| ShadowError::PatchRejected {
                path: overlay.file.clone(),
            })?
            .write_all(overlay.patch.as_bytes())
            .map_err(|_| ShadowError::PatchRejected {
                path: overlay.file.clone(),
            })?;
        let status = child.wait().map_err(|_| ShadowError::GitUnavailable)?;
        if !status.success() {
            return Err(ShadowError::PatchRejected {
                path: overlay.file.clone(),
            });
        }
    }
    Ok(())
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_bytes(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn is_exact_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_fails_closed_without_network_and_service_isolation_proof() {
        let policy = ShadowLaunchPolicy::default();
        assert_eq!(
            authorize_shadow_launch("127.0.0.1", &policy),
            Err(ShadowLaunchBlocker::NetworkIsolationUnproven)
        );
    }

    #[test]
    fn shadow_startup_and_lifetime_overrun_are_rejected_before_launch() {
        let mut policy = ShadowLaunchPolicy {
            network_isolation_proven: true,
            production_service_isolation_proven: true,
            ..Default::default()
        };
        policy.startup_timeout_ms = MAX_SHADOW_STARTUP_MS + 1;
        assert_eq!(
            authorize_shadow_launch("127.0.0.1", &policy),
            Err(ShadowLaunchBlocker::InvalidStartupTimeout)
        );

        policy.startup_timeout_ms = MAX_SHADOW_STARTUP_MS;
        policy.lifetime_ms = MAX_SHADOW_LIFETIME_MS + 1;
        assert_eq!(
            authorize_shadow_launch("127.0.0.1", &policy),
            Err(ShadowLaunchBlocker::InvalidLifetime)
        );
    }

    #[test]
    fn external_host_navigation_is_rejected() {
        let policy = ShadowLaunchPolicy {
            network_isolation_proven: true,
            production_service_isolation_proven: true,
            ..Default::default()
        };
        assert_eq!(
            authorize_shadow_launch("0.0.0.0", &policy),
            Err(ShadowLaunchBlocker::NonLoopbackRequested)
        );
    }

    #[test]
    fn loopback_port_is_dynamically_reserved() {
        let reservation = LoopbackPortReservation::reserve().unwrap();
        assert_ne!(reservation.port().unwrap(), 0);
    }

    #[test]
    fn patch_digest_is_deterministic_and_order_sensitive() {
        let overlay = SourceOverlay {
            file: "src/app.rs".into(),
            base_hash: "sha256:a".into(),
            patch: "patch".into(),
        };
        assert_eq!(
            patch_digest(std::slice::from_ref(&overlay)),
            patch_digest(std::slice::from_ref(&overlay))
        );
        let other = SourceOverlay {
            file: "src/other.rs".into(),
            ..overlay
        };
        assert_ne!(
            patch_digest(&[other.clone(), other.clone()]),
            patch_digest(&[other])
        );
    }

    #[test]
    fn secret_and_traversal_paths_are_rejected() {
        assert!(is_sensitive_path(".env"));
        assert!(is_sensitive_path("config/credentials.json"));
        assert!(validate_relative_path("../escape.rs").is_err());
        assert!(validate_relative_path("/absolute.rs").is_err());
    }
}
