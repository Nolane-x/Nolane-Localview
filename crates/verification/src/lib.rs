#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use localview_causal::{ProofCapsule, ProofVerdict};
use localview_content_addressed::object_hash;
use localview_contracts::{ContractResult, ContractVerdict};
use localview_evidence::{EvidenceId, EvidenceKind, EvidenceObject};
use localview_protocol::{VisualChangeExpectation, VisualDiffMetrics};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageTarget {
    pub id: String,
    pub risk_weight: u16,
    pub required_evidence_classes: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageObservation {
    pub target_id: String,
    pub evidence_classes: BTreeSet<String>,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoverageReport {
    pub denominator_known: bool,
    pub target_coverage: Option<f32>,
    pub weighted_coverage: Option<f32>,
    pub uncovered_targets: Vec<String>,
    pub weak_targets: Vec<String>,
}

pub fn coverage_report(
    targets: &[CoverageTarget],
    observations: &[CoverageObservation],
) -> CoverageReport {
    if targets.is_empty() {
        return CoverageReport {
            denominator_known: false,
            target_coverage: None,
            weighted_coverage: None,
            uncovered_targets: Vec::new(),
            weak_targets: Vec::new(),
        };
    }
    let by_target = observations
        .iter()
        .map(|observation| (observation.target_id.as_str(), observation))
        .collect::<BTreeMap<_, _>>();
    let total_weight = targets
        .iter()
        .map(|target| target.risk_weight.max(1) as u64)
        .sum::<u64>();
    let mut complete_count = 0usize;
    let mut covered_weight = 0u64;
    let mut uncovered = Vec::new();
    let mut weak = Vec::new();
    for target in targets {
        match by_target.get(target.id.as_str()) {
            None => uncovered.push(target.id.clone()),
            Some(observation) => {
                let complete = target
                    .required_evidence_classes
                    .is_subset(&observation.evidence_classes);
                if complete {
                    complete_count += 1;
                    covered_weight += target.risk_weight.max(1) as u64;
                } else {
                    weak.push(target.id.clone());
                    let required = target.required_evidence_classes.len().max(1) as u64;
                    let present = target
                        .required_evidence_classes
                        .intersection(&observation.evidence_classes)
                        .count() as u64;
                    covered_weight +=
                        (target.risk_weight.max(1) as u64 * present) / required;
                }
            }
        }
    }
    CoverageReport {
        denominator_known: true,
        target_coverage: Some(complete_count as f32 / targets.len() as f32),
        weighted_coverage: Some(covered_weight as f32 / total_weight as f32),
        uncovered_targets: uncovered,
        weak_targets: weak,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Discovered,
    Observed,
    Verified,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrictCoverageObservation {
    pub target_id: String,
    pub state: VerificationState,
    pub evidence_classes: BTreeSet<String>,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoverageTargetStatus {
    pub id: String,
    pub state: VerificationState,
    pub evidence_ids: Vec<EvidenceId>,
    pub missing_evidence_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrictCoverageReport {
    pub denominator_known: bool,
    pub verified_ratio: Option<f32>,
    pub risk_weighted_verified_ratio: Option<f32>,
    pub targets: Vec<CoverageTargetStatus>,
}

pub fn strict_coverage_report(
    targets: &[CoverageTarget],
    observations: &[StrictCoverageObservation],
) -> StrictCoverageReport {
    if targets.is_empty() {
        return StrictCoverageReport {
            denominator_known: false,
            verified_ratio: None,
            risk_weighted_verified_ratio: None,
            targets: Vec::new(),
        };
    }

    let by_target = observations
        .iter()
        .map(|observation| (observation.target_id.as_str(), observation))
        .collect::<BTreeMap<_, _>>();
    let total_weight = targets
        .iter()
        .map(|target| target.risk_weight.max(1) as u64)
        .sum::<u64>();
    let mut verified = 0usize;
    let mut verified_weight = 0u64;
    let mut statuses = Vec::with_capacity(targets.len());

    for target in targets {
        let Some(observation) = by_target.get(target.id.as_str()) else {
            statuses.push(CoverageTargetStatus {
                id: target.id.clone(),
                state: VerificationState::Unknown,
                evidence_ids: Vec::new(),
                missing_evidence_classes: target
                    .required_evidence_classes
                    .iter()
                    .cloned()
                    .collect(),
            });
            continue;
        };
        let missing = target
            .required_evidence_classes
            .difference(&observation.evidence_classes)
            .cloned()
            .collect::<Vec<_>>();
        let state = if observation.state == VerificationState::Stale {
            VerificationState::Stale
        } else if observation.state == VerificationState::Verified && missing.is_empty() {
            verified += 1;
            verified_weight += target.risk_weight.max(1) as u64;
            VerificationState::Verified
        } else {
            observation.state
        };
        statuses.push(CoverageTargetStatus {
            id: target.id.clone(),
            state,
            evidence_ids: observation.evidence_ids.clone(),
            missing_evidence_classes: missing,
        });
    }

    StrictCoverageReport {
        denominator_known: true,
        verified_ratio: Some(verified as f32 / targets.len() as f32),
        risk_weighted_verified_ratio: Some(verified_weight as f32 / total_weight as f32),
        targets: statuses,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckSample {
    pub run_id: String,
    pub outcome_hash: String,
    pub environment_hash: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeterminismReport {
    pub determinism_score: Option<f32>,
    pub flaky: bool,
    pub outcome_count: usize,
    pub comparable_samples: usize,
    pub reason: String,
}

pub fn determinism(samples: &[CheckSample]) -> DeterminismReport {
    if samples.len() < 2 {
        return DeterminismReport {
            determinism_score: None,
            flaky: false,
            outcome_count: samples.len(),
            comparable_samples: samples.len(),
            reason: "not enough repeated evidence to assign a determinism score".into(),
        };
    }
    let environment = &samples[0].environment_hash;
    let comparable = samples
        .iter()
        .filter(|sample| &sample.environment_hash == environment)
        .collect::<Vec<_>>();
    if comparable.len() < 2 {
        return DeterminismReport {
            determinism_score: None,
            flaky: false,
            outcome_count: comparable.len(),
            comparable_samples: comparable.len(),
            reason: "environment changed between runs".into(),
        };
    }
    let counts = comparable.iter().fold(
        BTreeMap::<&str, usize>::new(),
        |mut counts, sample| {
            *counts.entry(sample.outcome_hash.as_str()).or_default() += 1;
            counts
        },
    );
    let dominant = counts.values().copied().max().unwrap_or_default();
    let score = dominant as f32 / comparable.len() as f32;
    DeterminismReport {
        determinism_score: Some(score),
        flaky: counts.len() > 1,
        outcome_count: counts.len(),
        comparable_samples: comparable.len(),
        reason: if counts.len() > 1 {
            "same environment produced multiple outcomes".into()
        } else {
            "repeated outcome is stable in the sampled environment".into()
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StalenessContext {
    pub current_revision: String,
    pub current_environment_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceFreshness {
    pub evidence_id: EvidenceId,
    pub stale: bool,
    pub reasons: Vec<String>,
}

pub fn evidence_freshness(
    evidence: &EvidenceObject,
    context: &StalenessContext,
) -> EvidenceFreshness {
    let mut reasons = Vec::new();
    if evidence
        .provenance
        .revision
        .as_deref()
        .is_some_and(|revision| revision != context.current_revision)
    {
        reasons.push("revision changed".into());
    }
    if evidence.provenance.engine.as_deref().is_some_and(|engine| {
        engine.starts_with("env:")
            && engine.trim_start_matches("env:") != context.current_environment_hash
    }) {
        reasons.push("environment changed".into());
    }
    EvidenceFreshness {
        evidence_id: evidence.id.clone(),
        stale: !reasons.is_empty(),
        reasons,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProofQuality {
    pub score: f32,
    pub accepted: bool,
    pub reasons: Vec<String>,
}

pub fn assess_proof(
    proof: &ProofCapsule,
    evidence: &[EvidenceObject],
    contracts: &[ContractResult],
    minimum_confidence: f32,
) -> ProofQuality {
    let by_id = evidence
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let mut reasons = Vec::new();
    let missing = proof
        .evidence_ids
        .iter()
        .filter(|id| !by_id.contains_key(id.as_str()))
        .count();
    if missing > 0 {
        reasons.push(format!("{missing} proof evidence object(s) are missing"));
    }
    let tainted = proof
        .evidence_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()))
        .filter(|item| item.secret_taint)
        .count();
    if tainted > 0 {
        reasons.push(format!(
            "{tainted} proof evidence object(s) contain secret taint"
        ));
    }
    let confidences = proof
        .evidence_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()))
        .map(|item| item.confidence)
        .collect::<Vec<_>>();
    let mean_confidence = if confidences.is_empty() {
        0.0
    } else {
        confidences.iter().copied().sum::<f32>() / confidences.len() as f32
    };
    if mean_confidence < minimum_confidence {
        reasons.push(format!(
            "mean evidence confidence {mean_confidence:.2} is below required {minimum_confidence:.2}"
        ));
    }
    let hard_contract_failures = contracts
        .iter()
        .filter(|result| result.verdict == ContractVerdict::Fail)
        .count();
    if hard_contract_failures > 0 {
        reasons.push(format!(
            "{hard_contract_failures} executable contract(s) failed"
        ));
    }
    let verdict_factor = match proof.verdict {
        ProofVerdict::Pass => 1.0,
        ProofVerdict::Inconclusive => 0.5,
        ProofVerdict::Stale => 0.2,
        ProofVerdict::Fail => 0.0,
    };
    let completeness = if proof.evidence_ids.is_empty() {
        0.0
    } else {
        proof.evidence_ids.len().saturating_sub(missing) as f32
            / proof.evidence_ids.len() as f32
    };
    let score = (verdict_factor * 0.35 + completeness * 0.25 + mean_confidence * 0.4)
        .clamp(0.0, 1.0);
    let accepted = proof.verdict == ProofVerdict::Pass
        && missing == 0
        && tainted == 0
        && hard_contract_failures == 0
        && mean_confidence >= minimum_confidence;
    ProofQuality {
        score,
        accepted,
        reasons,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegressionDecision {
    pub reject: bool,
    pub reasons: Vec<String>,
}

pub fn regression_decision(
    baseline_failures: &BTreeSet<String>,
    candidate_failures: &BTreeSet<String>,
    proof_quality: &ProofQuality,
) -> RegressionDecision {
    let new_failures = candidate_failures
        .difference(baseline_failures)
        .cloned()
        .collect::<Vec<_>>();
    let mut reasons = new_failures
        .iter()
        .map(|failure| format!("new failure: {failure}"))
        .collect::<Vec<_>>();
    if !proof_quality.accepted {
        reasons.push("candidate does not carry an acceptable proof".into());
    }
    RegressionDecision {
        reject: !new_failures.is_empty() || !proof_quality.accepted,
        reasons,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiveVerificationVerdict {
    Pass,
    Fail,
    Inconclusive,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisualChangeDecision {
    pub verdict: LiveVerificationVerdict,
    pub changed_pixels: u64,
    pub changed_ratio: f64,
    pub reason: String,
}

pub fn verify_visual_change(
    expectation: &VisualChangeExpectation,
    metrics: &VisualDiffMetrics,
) -> VisualChangeDecision {
    if !valid_ratio(metrics.changed_ratio) {
        return VisualChangeDecision {
            verdict: LiveVerificationVerdict::Inconclusive,
            changed_pixels: metrics.changed_pixels,
            changed_ratio: metrics.changed_ratio,
            reason: "invalid visual diff metrics".into(),
        };
    }

    let (verdict, reason) = match expectation {
        VisualChangeExpectation::Unchanged { max_changed_ratio } => {
            if !valid_ratio(*max_changed_ratio) {
                (
                    LiveVerificationVerdict::Inconclusive,
                    "invalid unchanged visual expectation".to_string(),
                )
            } else if metrics.changed_ratio <= *max_changed_ratio {
                (
                    LiveVerificationVerdict::Pass,
                    format!(
                        "changed ratio {:.6} is within maximum {:.6}",
                        metrics.changed_ratio, max_changed_ratio
                    ),
                )
            } else {
                (
                    LiveVerificationVerdict::Fail,
                    format!(
                        "changed ratio {:.6} exceeds maximum {:.6}",
                        metrics.changed_ratio, max_changed_ratio
                    ),
                )
            }
        }
        VisualChangeExpectation::Changed { min_changed_ratio } => {
            if !valid_ratio(*min_changed_ratio) {
                (
                    LiveVerificationVerdict::Inconclusive,
                    "invalid changed visual expectation".to_string(),
                )
            } else if metrics.changed_ratio >= *min_changed_ratio {
                (
                    LiveVerificationVerdict::Pass,
                    format!(
                        "changed ratio {:.6} meets minimum {:.6}",
                        metrics.changed_ratio, min_changed_ratio
                    ),
                )
            } else {
                (
                    LiveVerificationVerdict::Fail,
                    format!(
                        "changed ratio {:.6} is below minimum {:.6}",
                        metrics.changed_ratio, min_changed_ratio
                    ),
                )
            }
        }
    };

    VisualChangeDecision {
        verdict,
        changed_pixels: metrics.changed_pixels,
        changed_ratio: metrics.changed_ratio,
        reason,
    }
}

fn valid_ratio(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveVerificationPacket {
    pub revision: Option<String>,
    pub verdict: LiveVerificationVerdict,
    pub required_evidence_classes: BTreeSet<String>,
    pub fresh_evidence_classes: BTreeSet<String>,
    pub missing_evidence_classes: Vec<String>,
    pub fresh_evidence_ids: Vec<EvidenceId>,
    pub stale_evidence_ids: Vec<EvidenceId>,
    pub unbound_evidence_ids: Vec<EvidenceId>,
    pub tainted_evidence_ids: Vec<EvidenceId>,
    pub deterministic_failures: usize,
    pub critical_unknowns: usize,
    pub reasons: Vec<String>,
}

pub fn verify_current(
    revision: Option<&str>,
    evidence: &[EvidenceObject],
    deterministic_failures: usize,
    critical_unknowns: usize,
    required_evidence_classes: &BTreeSet<String>,
) -> LiveVerificationPacket {
    let mut fresh_classes = BTreeSet::new();
    let mut stale_classes = BTreeSet::new();
    let mut fresh_ids = Vec::new();
    let mut stale_ids = Vec::new();
    let mut unbound_ids = Vec::new();
    let mut tainted_ids = Vec::new();

    for item in evidence {
        if item.secret_taint {
            tainted_ids.push(item.id.clone());
        }
        match (revision, item.provenance.revision.as_deref()) {
            (Some(current), Some(bound)) if current == bound => {
                fresh_classes.insert(evidence_class(item.kind).to_owned());
                fresh_ids.push(item.id.clone());
            }
            (Some(_), Some(_)) => {
                stale_classes.insert(evidence_class(item.kind).to_owned());
                stale_ids.push(item.id.clone());
            }
            _ => unbound_ids.push(item.id.clone()),
        }
    }

    let missing = required_evidence_classes
        .difference(&fresh_classes)
        .cloned()
        .collect::<Vec<_>>();
    let mut reasons = Vec::new();
    let verdict = if deterministic_failures > 0 {
        reasons.push(format!(
            "{deterministic_failures} deterministic failure(s) are currently observed"
        ));
        LiveVerificationVerdict::Fail
    } else if revision.is_none() {
        reasons.push("verification has no source revision binding".into());
        LiveVerificationVerdict::Inconclusive
    } else if required_evidence_classes.is_empty() {
        reasons.push("verification plan declares no required evidence classes".into());
        LiveVerificationVerdict::Inconclusive
    } else if !missing.is_empty() {
        reasons.push(format!(
            "missing fresh evidence classes: {}",
            missing.join(", ")
        ));
        if missing.iter().any(|class| stale_classes.contains(class)) {
            LiveVerificationVerdict::Stale
        } else {
            LiveVerificationVerdict::Inconclusive
        }
    } else if fresh_ids
        .iter()
        .any(|id| tainted_ids.iter().any(|tainted| tainted == id))
    {
        reasons.push("fresh verification evidence carries secret taint".into());
        LiveVerificationVerdict::Inconclusive
    } else if critical_unknowns > 0 {
        reasons.push(format!(
            "{critical_unknowns} critical uncertainty item(s) remain unresolved"
        ));
        LiveVerificationVerdict::Inconclusive
    } else {
        reasons.push("all required evidence is fresh for the bound revision".into());
        LiveVerificationVerdict::Pass
    };

    LiveVerificationPacket {
        revision: revision.map(str::to_owned),
        verdict,
        required_evidence_classes: required_evidence_classes.clone(),
        fresh_evidence_classes: fresh_classes,
        missing_evidence_classes: missing,
        fresh_evidence_ids: fresh_ids,
        stale_evidence_ids: stale_ids,
        unbound_evidence_ids: unbound_ids,
        tainted_evidence_ids: tainted_ids,
        deterministic_failures,
        critical_unknowns,
        reasons,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationProofPayload {
    pub revision: Option<String>,
    pub verdict: LiveVerificationVerdict,
    pub evidence_ids: Vec<EvidenceId>,
    pub required_evidence_classes: BTreeSet<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerificationProof {
    pub proof_hash: String,
    pub payload: VerificationProofPayload,
}

pub fn proof_from_verification(packet: &LiveVerificationPacket) -> VerificationProof {
    let payload = VerificationProofPayload {
        revision: packet.revision.clone(),
        verdict: packet.verdict,
        evidence_ids: packet.fresh_evidence_ids.clone(),
        required_evidence_classes: packet.required_evidence_classes.clone(),
        reasons: packet.reasons.clone(),
    };
    let proof_hash = object_hash(&payload);
    VerificationProof {
        proof_hash,
        payload,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofStaleness {
    pub stale: bool,
    pub reasons: Vec<String>,
}

pub fn proof_staleness(proof: &VerificationProof, current_revision: Option<&str>) -> ProofStaleness {
    let mut reasons = Vec::new();
    match (proof.payload.revision.as_deref(), current_revision) {
        (Some(bound), Some(current)) if bound != current => {
            reasons.push("source revision changed since proof generation".into());
        }
        (None, _) => reasons.push("proof has no source revision binding".into()),
        (_, None) => reasons.push("current source revision is unavailable".into()),
        _ => {}
    }
    ProofStaleness {
        stale: !reasons.is_empty(),
        reasons,
    }
}

pub fn evidence_class(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Visual => "visual",
        EvidenceKind::Semantic => "semantic",
        EvidenceKind::Layout => "layout",
        EvidenceKind::Console => "console",
        EvidenceKind::Network => "network",
        EvidenceKind::Source => "source",
        EvidenceKind::Interaction => "interaction",
        EvidenceKind::Performance => "performance",
        EvidenceKind::Accessibility => "accessibility",
        EvidenceKind::Contract => "contract",
        EvidenceKind::Test => "test",
        EvidenceKind::Causal => "causal",
        EvidenceKind::Coverage => "coverage",
        EvidenceKind::Proof => "proof",
        EvidenceKind::Repro => "repro",
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use localview_evidence::{EvidenceDraft, Provenance, UncertaintyClass};
    use uuid::Uuid;

    use super::*;

    fn evidence(kind: EvidenceKind, revision: Option<&str>) -> EvidenceObject {
        let draft = EvidenceDraft {
            kind,
            session_id: Uuid::nil(),
            region: None,
            payload: serde_json::Value::Null,
            provenance: Provenance {
                source: "test".into(),
                engine: Some("native".into()),
                revision: revision.map(str::to_owned),
                parent_ids: Vec::new(),
                captured_at: DateTime::<Utc>::from_timestamp(1, 0).expect("timestamp"),
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        };
        EvidenceObject {
            id: localview_evidence::evidence_id(&draft),
            kind: draft.kind,
            session_id: draft.session_id,
            region: draft.region,
            payload: draft.payload,
            provenance: draft.provenance,
            confidence: draft.confidence,
            uncertainty: draft.uncertainty,
            secret_taint: draft.secret_taint,
        }
    }

    #[test]
    fn unknown_denominator_never_becomes_fake_one_hundred_percent() {
        let report = coverage_report(&[], &[]);
        assert!(!report.denominator_known);
        assert_eq!(report.target_coverage, None);
        assert_eq!(report.weighted_coverage, None);
    }

    #[test]
    fn repeated_different_hashes_in_same_environment_are_flaky() {
        let samples = vec![
            CheckSample {
                run_id: "1".into(),
                outcome_hash: "a".into(),
                environment_hash: "env".into(),
                passed: true,
            },
            CheckSample {
                run_id: "2".into(),
                outcome_hash: "b".into(),
                environment_hash: "env".into(),
                passed: false,
            },
        ];
        let report = determinism(&samples);
        assert!(report.flaky);
        assert_eq!(report.determinism_score, Some(0.5));
    }

    #[test]
    fn weighted_coverage_penalizes_missing_high_risk_target() {
        let targets = vec![
            CoverageTarget {
                id: "checkout".into(),
                risk_weight: 10,
                required_evidence_classes: BTreeSet::from(["behavior".into()]),
            },
            CoverageTarget {
                id: "footer".into(),
                risk_weight: 1,
                required_evidence_classes: BTreeSet::from(["visual".into()]),
            },
        ];
        let observations = vec![CoverageObservation {
            target_id: "footer".into(),
            evidence_classes: BTreeSet::from(["visual".into()]),
            evidence_ids: vec!["ev".into()],
        }];
        let report = coverage_report(&targets, &observations);
        assert!(report.weighted_coverage.is_some_and(|value| value < 0.2));
        assert_eq!(report.uncovered_targets, vec!["checkout"]);
    }

    #[test]
    fn observed_is_not_verified_in_strict_coverage() {
        let target = CoverageTarget {
            id: "checkout".into(),
            risk_weight: 10,
            required_evidence_classes: BTreeSet::from(["semantic".into()]),
        };
        let observation = StrictCoverageObservation {
            target_id: "checkout".into(),
            state: VerificationState::Observed,
            evidence_classes: BTreeSet::from(["semantic".into()]),
            evidence_ids: vec!["ev".into()],
        };
        let report = strict_coverage_report(&[target], &[observation]);
        assert_eq!(report.verified_ratio, Some(0.0));
        assert_eq!(report.targets[0].state, VerificationState::Observed);
    }

    #[test]
    fn verification_cannot_pass_with_unbound_evidence() {
        let required = BTreeSet::from(["semantic".into(), "layout".into()]);
        let packet = verify_current(
            Some("wt:abc"),
            &[evidence(EvidenceKind::Semantic, None), evidence(EvidenceKind::Layout, None)],
            0,
            0,
            &required,
        );
        assert_eq!(packet.verdict, LiveVerificationVerdict::Inconclusive);
        assert_eq!(packet.unbound_evidence_ids.len(), 2);
    }

    #[test]
    fn verification_pass_requires_fresh_revision_bound_evidence() {
        let required = BTreeSet::from(["semantic".into(), "layout".into()]);
        let packet = verify_current(
            Some("wt:abc"),
            &[
                evidence(EvidenceKind::Semantic, Some("wt:abc")),
                evidence(EvidenceKind::Layout, Some("wt:abc")),
            ],
            0,
            0,
            &required,
        );
        assert_eq!(packet.verdict, LiveVerificationVerdict::Pass);
        let proof = proof_from_verification(&packet);
        assert!(proof.proof_hash.starts_with("sha256:"));
        assert!(!proof_staleness(&proof, Some("wt:abc")).stale);
        assert!(proof_staleness(&proof, Some("wt:def")).stale);
    }
}
