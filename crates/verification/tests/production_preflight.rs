use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use localview_counterfactual::{
    CounterfactualCandidate, ExternalSideEffectContainment, IsolationLevel, SourceOverlay,
    sha256_bytes,
};
use localview_verification::{
    ProductionCandidatePreflightVerdict, run_production_candidate_preflight,
};
use uuid::Uuid;

fn temp_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "localview-wave9-production-preflight-{}-{stamp}",
        std::process::id()
    ))
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git available");
    assert!(
        output.status.success(),
        "git command failed: {args:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn production_preflight_uses_real_temp_git_shadow_without_mutating_source() {
    let root = temp_path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/app.txt"), b"before\n").unwrap();
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "wave9@local.invalid"]);
    git(&root, &["config", "user.name", "Wave9 Production Preflight"]);
    git(&root, &["add", "src/app.txt"]);
    git(&root, &["commit", "-m", "base"]);
    let head = git(&root, &["rev-parse", "HEAD"]);

    let patch = "diff --git a/src/app.txt b/src/app.txt\n--- a/src/app.txt\n+++ b/src/app.txt\n@@ -1 +1 @@\n-before\n+after\n";
    let candidate = CounterfactualCandidate {
        id: Uuid::new_v4(),
        name: "production-preflight".into(),
        base_revision: head.clone(),
        overlays: vec![SourceOverlay {
            file: "src/app.txt".into(),
            base_hash: sha256_bytes(b"before\n"),
            patch: patch.into(),
        }],
        isolation: IsolationLevel::SemanticOnly,
        disposable: true,
        evidence_ids: vec!["integration:proposal".into()],
        metrics: BTreeMap::new(),
        hard_failures: BTreeSet::new(),
    };

    let receipt = run_production_candidate_preflight(&root, &candidate).unwrap();

    assert_eq!(receipt.candidate_id, candidate.id.to_string());
    assert_eq!(receipt.base_revision, head);
    assert_eq!(
        receipt.shadow_proof.external_side_effect_containment,
        ExternalSideEffectContainment::NotProven
    );
    assert_eq!(
        receipt.verdict,
        ProductionCandidatePreflightVerdict::Inconclusive
    );
    assert!(receipt.cleanup_proof.attempted);
    assert!(receipt.cleanup_proof.worktree_removed);
    assert!(receipt.cleanup_proof.directory_absent);
    assert_eq!(fs::read(root.join("src/app.txt")).unwrap(), b"before\n");
    assert!(
        !Path::new(&receipt.shadow_proof.shadow_path).exists(),
        "shadow directory must be absent after preflight"
    );
    assert!(
        receipt
            .reasons
            .iter()
            .any(|reason| reason.contains("containment is not proven"))
    );

    let worktrees = git(&root, &["worktree", "list", "--porcelain"]);
    assert_eq!(
        worktrees.lines().filter(|line| line.starts_with("worktree ")).count(),
        1
    );

    fs::remove_dir_all(root).unwrap();
}
