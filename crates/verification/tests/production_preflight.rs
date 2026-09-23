use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use localview_counterfactual::{
    CounterfactualCandidate, ExternalSideEffectContainment, IsolationLevel, SourceOverlay,
    sha256_bytes,
};
use localview_verification::{
    AutonomousVerificationVerdict, ImpactKind, ProductionCandidatePreflightVerdict,
    ProductionObservedVerificationInput, bind_production_affected_state,
    build_production_observation_receipt, run_production_candidate_preflight,
};
use uuid::Uuid;

fn temp_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localview-wave9-production-preflight-{}-{}",
        std::process::id(),
        Uuid::new_v4()
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
    let receipt = bind_production_affected_state(
        receipt,
        &candidate,
        "http://127.0.0.1:5173/settings",
        Some("@e1"),
    )
    .unwrap();

    let candidate_id = candidate.id.to_string();
    assert_eq!(receipt.candidate_id.as_deref(), Some(candidate_id.as_str()));
    assert_eq!(receipt.base_revision.as_deref(), Some(head.as_str()));
    let shadow_proof = receipt.shadow_proof.as_ref().expect("shadow proof");
    let cleanup_proof = receipt.cleanup_proof.as_ref().expect("cleanup proof");
    assert_eq!(
        shadow_proof.external_side_effect_containment,
        ExternalSideEffectContainment::NotProven
    );
    assert_eq!(
        receipt.verdict,
        ProductionCandidatePreflightVerdict::Inconclusive
    );
    assert!(receipt.affected_state_plan_hash.is_some());
    assert!(receipt.affected_state_plan.is_some());
    assert!(
        receipt
            .affected_state_incomplete_reasons
            .iter()
            .any(|reason| reason.contains("denominator is unknown"))
    );
    let predicted = receipt.predicted_impact.as_ref().expect("predicted impact");
    assert!(
        predicted
            .targets
            .iter()
            .any(|target| target.kind == ImpactKind::Route
                && target.id == "http://127.0.0.1:5173/settings")
    );
    assert!(
        predicted
            .targets
            .iter()
            .any(|target| target.kind == ImpactKind::Reference && target.id == "@e1")
    );
    assert!(cleanup_proof.attempted);
    assert!(cleanup_proof.worktree_removed);
    assert!(cleanup_proof.directory_absent);
    assert_eq!(fs::read(root.join("src/app.txt")).unwrap(), b"before\n");
    assert!(
        !Path::new(&shadow_proof.shadow_path).exists(),
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


#[test]
fn live_production_observation_receipt_stays_inconclusive_until_remaining_authorities_exist() {
    let root = temp_path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/app.txt"), b"before\n").unwrap();
    git(&root, &["init"]);
    git(&root, &["config", "user.email", "wave9@local.invalid"]);
    git(&root, &["config", "user.name", "Wave9 Production Observation"]);
    git(&root, &["add", "src/app.txt"]);
    git(&root, &["commit", "-m", "base"]);
    let head = git(&root, &["rev-parse", "HEAD"]);

    let candidate = CounterfactualCandidate {
        id: Uuid::new_v4(),
        name: "production-observation".into(),
        base_revision: head,
        overlays: vec![SourceOverlay {
            file: "src/app.txt".into(),
            base_hash: sha256_bytes(b"before\n"),
            patch: "diff --git a/src/app.txt b/src/app.txt\n--- a/src/app.txt\n+++ b/src/app.txt\n@@ -1 +1 @@\n-before\n+after\n".into(),
        }],
        isolation: IsolationLevel::SemanticOnly,
        disposable: true,
        evidence_ids: vec!["integration:proposal".into()],
        metrics: BTreeMap::new(),
        hard_failures: BTreeSet::new(),
    };
    let preflight = bind_production_affected_state(
        run_production_candidate_preflight(&root, &candidate).unwrap(),
        &candidate,
        "http://127.0.0.1:5173/settings",
        Some("@e1"),
    )
    .unwrap();

    let receipt = build_production_observation_receipt(
        &preflight,
        ProductionObservedVerificationInput {
            canonical_route: "http://127.0.0.1:5173/settings".into(),
            reference: Some("@e1".into()),
            reference_changed: true,
            visual_region_count: 1,
            regression_signals: Vec::new(),
            evidence_ids: vec!["visual:after".into()],
            observed_runtime_ms: 250,
        },
    )
    .unwrap();

    assert_eq!(receipt.final_verdict, AutonomousVerificationVerdict::Inconclusive);
    assert_eq!(
        receipt.external_side_effect_containment,
        ExternalSideEffectContainment::NotProven
    );
    assert!(
        receipt
            .actual_impact
            .targets
            .iter()
            .any(|target| target.kind == ImpactKind::Reference && target.id == "@e1")
    );
    assert!(
        receipt
            .reasons
            .iter()
            .any(|reason| reason.contains("contract catalog execution"))
    );
    assert!(
        receipt
            .reasons
            .iter()
            .any(|reason| reason.contains("mutation challenges"))
    );
    assert!(!receipt.resource_budget.executed_states.eq(&0));

    fs::remove_dir_all(root).unwrap();
}
