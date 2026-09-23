use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use localview_counterfactual::{
    CounterfactualCandidate, ExternalSideEffectContainment, IsolationLevel, MAX_SHADOW_PATCH_BYTES,
    ShadowError, ShadowWorkspace, SourceOverlay, patch_digest, sha256_bytes,
};
use uuid::Uuid;

struct Fixture {
    root: PathBuf,
    head: String,
    base_bytes: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let root = temp_path("fixture");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/app.txt"), b"before\n").unwrap();
        git(&root, &["init"]);
        git(&root, &["config", "user.email", "wave9@local.invalid"]);
        git(&root, &["config", "user.name", "Wave9 Fixture"]);
        git(&root, &["add", "src/app.txt"]);
        git(&root, &["commit", "-m", "base"]);
        let head = output(&root, &["rev-parse", "HEAD"]).trim().to_owned();
        Self {
            root,
            head,
            base_bytes: b"before\n".to_vec(),
        }
    }

    fn overlay(&self, path: &str, patch: String) -> SourceOverlay {
        SourceOverlay {
            file: path.into(),
            base_hash: sha256_bytes(&self.base_bytes),
            patch,
        }
    }

    fn candidate(&self, overlay: SourceOverlay) -> CounterfactualCandidate {
        CounterfactualCandidate {
            id: Uuid::new_v4(),
            name: "fixture".into(),
            base_revision: self.head.clone(),
            overlays: vec![overlay],
            isolation: IsolationLevel::SemanticOnly,
            disposable: true,
            evidence_ids: vec!["fixture-evidence".into()],
            metrics: BTreeMap::new(),
            hard_failures: BTreeSet::new(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn normal_patch(path: &str, before: &str, after: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-{before}\n+{after}\n"
    )
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-wave9-{label}-{}-{}",
        std::process::id(),
        Uuid::new_v4()
    ))
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git command failed: {args:?}");
}

fn output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git command failed: {args:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn real_isolated_shadow_candidate_never_mutates_dirty_worktree_and_cleans_up() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("dirty.txt"), "keep-me\n").unwrap();
    let before_status = output(&fixture.root, &["status", "--porcelain=v1"]);
    let patch = normal_patch("src/app.txt", "before", "after");
    let overlay = fixture.overlay("src/app.txt", patch);
    let candidate = fixture.candidate(overlay.clone());

    assert_eq!(patch_digest(&candidate.overlays), patch_digest(&[overlay]));
    let mut shadow = ShadowWorkspace::prepare(&fixture.root, &candidate).unwrap();

    assert_eq!(
        fs::read_to_string(shadow.root().join("src/app.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "after\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/app.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "before\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("dirty.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "keep-me\n"
    );

    let proof = shadow.proof().unwrap();
    assert!(proof.original_worktree_dirty);
    assert!(proof.real_worktree_unchanged);
    assert_eq!(
        proof.external_side_effect_containment,
        ExternalSideEffectContainment::NotProven
    );
    assert_eq!(proof.base_revision, fixture.head);
    assert_eq!(proof.changed_files, vec!["src/app.txt"]);
    assert!(Path::new(&proof.shadow_path).exists());

    let shadow_path = shadow.root().to_path_buf();
    let cleanup = shadow.cleanup().unwrap();
    assert!(cleanup.attempted);
    assert!(cleanup.worktree_removed);
    assert!(cleanup.directory_absent);
    assert!(!shadow_path.exists());
    assert_eq!(
        output(&fixture.root, &["status", "--porcelain=v1"]),
        before_status
    );
}

#[test]
fn executable_shadow_levels_fail_closed_until_runtime_isolation_is_proven() {
    let fixture = Fixture::new();
    let overlay = fixture.overlay(
        "src/app.txt",
        normal_patch("src/app.txt", "before", "after"),
    );
    for isolation in [
        IsolationLevel::NativeWebView,
        IsolationLevel::ChromiumSandbox,
    ] {
        let mut candidate = fixture.candidate(overlay.clone());
        candidate.isolation = isolation;
        assert_eq!(
            ShadowWorkspace::prepare(&fixture.root, &candidate).unwrap_err(),
            ShadowError::UnsupportedIsolation
        );
    }
}

#[cfg(unix)]
#[test]
fn shadow_preparation_does_not_run_repository_checkout_hooks() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let hook = fixture.root.join(".git/hooks/post-checkout");
    let sentinel = fixture.root.join("hook-fired");
    fs::write(
        &hook,
        format!("#!/bin/sh\nprintf fired > '{}'\n", sentinel.display()),
    )
    .unwrap();
    let mut permissions = fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).unwrap();

    let candidate = fixture.candidate(fixture.overlay(
        "src/app.txt",
        normal_patch("src/app.txt", "before", "after"),
    ));
    let mut shadow = ShadowWorkspace::prepare(&fixture.root, &candidate).unwrap();
    assert!(!sentinel.exists());
    shadow.cleanup().unwrap();
    assert!(!sentinel.exists());
}

#[test]
fn base_revision_mismatch_fails_before_shadow_creation() {
    let fixture = Fixture::new();
    let mut candidate = fixture.candidate(fixture.overlay(
        "src/app.txt",
        normal_patch("src/app.txt", "before", "after"),
    ));
    candidate.base_revision = "0000000000000000000000000000000000000000".into();
    assert!(matches!(
        ShadowWorkspace::prepare(&fixture.root, &candidate),
        Err(ShadowError::BaseRevisionMismatch { .. })
    ));
}

#[test]
fn traversal_secret_oversized_and_binary_candidates_fail_closed() {
    let fixture = Fixture::new();

    let traversal = fixture.candidate(SourceOverlay {
        file: "../escape.txt".into(),
        base_hash: sha256_bytes(b"before\n"),
        patch: normal_patch("../escape.txt", "before", "after"),
    });
    assert!(matches!(
        ShadowWorkspace::prepare(&fixture.root, &traversal),
        Err(ShadowError::InvalidPath { .. })
    ));

    let secret = fixture.candidate(SourceOverlay {
        file: ".env".into(),
        base_hash: sha256_bytes(b"before\n"),
        patch: normal_patch(".env", "before", "after"),
    });
    assert!(matches!(
        ShadowWorkspace::prepare(&fixture.root, &secret),
        Err(ShadowError::SecretFile { .. })
    ));

    let oversized = fixture.candidate(SourceOverlay {
        file: "src/app.txt".into(),
        base_hash: sha256_bytes(b"before\n"),
        patch: "x".repeat(MAX_SHADOW_PATCH_BYTES + 1),
    });
    assert_eq!(
        ShadowWorkspace::prepare(&fixture.root, &oversized).unwrap_err(),
        ShadowError::OversizedPatch
    );

    fs::write(fixture.root.join("src/binary.bin"), [0, 1, 2, 3]).unwrap();
    git(&fixture.root, &["add", "src/binary.bin"]);
    git(&fixture.root, &["commit", "-m", "binary"]);
    let binary_head = output(&fixture.root, &["rev-parse", "HEAD"])
        .trim()
        .to_owned();
    let binary = CounterfactualCandidate {
        id: Uuid::new_v4(),
        name: "binary".into(),
        base_revision: binary_head,
        overlays: vec![SourceOverlay {
            file: "src/binary.bin".into(),
            base_hash: sha256_bytes(&[0, 1, 2, 3]),
            patch: "diff --git a/src/binary.bin b/src/binary.bin\n--- a/src/binary.bin\n+++ b/src/binary.bin\n@@ -1 +1 @@\n-a\n+b\n".into(),
        }],
        isolation: IsolationLevel::SemanticOnly,
        disposable: true,
        evidence_ids: vec![],
        metrics: BTreeMap::new(),
        hard_failures: BTreeSet::new(),
    };
    assert!(matches!(
        ShadowWorkspace::prepare(&fixture.root, &binary),
        Err(ShadowError::UnsupportedBinaryFile { .. })
    ));
}

#[test]
fn tracked_symlink_is_rejected_without_following_it() {
    let fixture = Fixture::new();
    // Create a symbolic-link tree entry portably without requiring the host to
    // permit filesystem symlink creation.
    let hash_output = Command::new("git")
        .arg("-C")
        .arg(&fixture.root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(b"app.txt")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(hash_output.status.success());
    let blob = String::from_utf8(hash_output.stdout)
        .unwrap()
        .trim()
        .to_owned();
    git(
        &fixture.root,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},src/link.txt"),
        ],
    );
    git(&fixture.root, &["commit", "-m", "symlink"]);
    let head = output(&fixture.root, &["rev-parse", "HEAD"])
        .trim()
        .to_owned();

    let candidate = CounterfactualCandidate {
        id: Uuid::new_v4(),
        name: "symlink".into(),
        base_revision: head,
        overlays: vec![SourceOverlay {
            file: "src/link.txt".into(),
            base_hash: sha256_bytes(b"app.txt"),
            patch: normal_patch("src/link.txt", "app.txt", "other.txt"),
        }],
        isolation: IsolationLevel::SemanticOnly,
        disposable: true,
        evidence_ids: vec![],
        metrics: BTreeMap::new(),
        hard_failures: BTreeSet::new(),
    };
    assert!(matches!(
        ShadowWorkspace::prepare(&fixture.root, &candidate),
        Err(ShadowError::SymlinkEscape { .. })
    ));
}

#[test]
fn non_git_project_fails_closed() {
    let root = temp_path("non-git");
    fs::create_dir_all(&root).unwrap();
    let candidate = CounterfactualCandidate {
        id: Uuid::new_v4(),
        name: "non-git".into(),
        base_revision: "0000000000000000000000000000000000000000".into(),
        overlays: vec![SourceOverlay {
            file: "x.txt".into(),
            base_hash: sha256_bytes(b"x"),
            patch: normal_patch("x.txt", "x", "y"),
        }],
        isolation: IsolationLevel::SemanticOnly,
        disposable: true,
        evidence_ids: vec![],
        metrics: BTreeMap::new(),
        hard_failures: BTreeSet::new(),
    };
    assert_eq!(
        ShadowWorkspace::prepare(&root, &candidate).unwrap_err(),
        ShadowError::NonGitProject
    );
    let _ = fs::remove_dir_all(root);
}
