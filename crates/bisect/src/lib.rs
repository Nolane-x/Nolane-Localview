#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevisionCandidate {
    pub revision: String,
    pub parent: Option<String>,
    pub changed_files: BTreeSet<String>,
    pub environment_hash: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProbeVerdict { Good, Bad, Skip, Inconclusive }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectProbe {
    pub revision: String,
    pub verdict: ProbeVerdict,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectContract {
    pub target: String,
    pub good_revision: String,
    pub bad_revision: String,
    pub environment_hash: String,
    pub relevant_files: BTreeSet<String>,
    pub allow_working_tree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectStep {
    pub revision: String,
    pub reason: String,
}

pub fn plan(
    history: &[RevisionCandidate],
    contract: &BisectContract,
    probes: &BTreeMap<String, ProbeVerdict>,
) -> Option<BisectStep> {
    let positions = history.iter().enumerate().map(|(index, revision)| (revision.revision.as_str(), index)).collect::<BTreeMap<_, _>>();
    let good = *positions.get(contract.good_revision.as_str())?;
    let bad = *positions.get(contract.bad_revision.as_str())?;
    let (start, end) = if good < bad { (good, bad) } else { (bad, good) };
    let candidates = history[start + 1..end].iter().filter(|revision| {
        revision.environment_hash == contract.environment_hash
            && probes.get(&revision.revision).is_none_or(|verdict| matches!(verdict, ProbeVerdict::Skip | ProbeVerdict::Inconclusive))
    }).filter(|revision| contract.relevant_files.is_empty() || !revision.changed_files.is_disjoint(&contract.relevant_files)).collect::<Vec<_>>();
    if candidates.is_empty() { return None; }
    let middle = candidates[candidates.len() / 2];
    Some(BisectStep { revision: middle.revision.clone(), reason: if contract.relevant_files.is_empty() { "midpoint of unresolved revision range".into() } else { "impact-aware midpoint touching relevant files".into() } })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectResult {
    pub first_bad: Option<String>,
    pub last_good: Option<String>,
    pub culprit_files: BTreeSet<String>,
    pub complete: bool,
    pub reasons: Vec<String>,
}

pub fn analyze(history: &[RevisionCandidate], contract: &BisectContract, probes: &[BisectProbe]) -> BisectResult {
    let probe_map = probes.iter().map(|probe| (probe.revision.as_str(), probe.verdict)).collect::<BTreeMap<_, _>>();
    let ordered = history.iter().filter(|revision| revision.environment_hash == contract.environment_hash).collect::<Vec<_>>();
    let first_bad_index = ordered.iter().position(|revision| probe_map.get(revision.revision.as_str()) == Some(&ProbeVerdict::Bad));
    let last_good_index = first_bad_index.and_then(|bad| (0..bad).rev().find(|index| probe_map.get(ordered[*index].revision.as_str()) == Some(&ProbeVerdict::Good)));
    let first_bad = first_bad_index.map(|index| ordered[index].revision.clone());
    let last_good = last_good_index.map(|index| ordered[index].revision.clone());
    let culprit_files = first_bad_index.and_then(|index| ordered.get(index)).map(|revision| revision.changed_files.intersection(&contract.relevant_files).cloned().collect()).unwrap_or_default();
    let mut reasons = Vec::new();
    if first_bad.is_none() { reasons.push("no verified bad revision in the pinned environment".into()); }
    if first_bad.is_some() && last_good.is_none() { reasons.push("no verified good predecessor in the pinned environment".into()); }
    let complete = first_bad.is_some() && last_good.is_some();
    BisectResult { first_bad, last_good, culprit_files, complete, reasons }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchStackEntry { pub id: String, pub base_revision: String, pub touched_files: BTreeSet<String> }

pub fn patch_stack_candidates<'a>(stack: &'a [PatchStackEntry], relevant_files: &BTreeSet<String>) -> Vec<&'a PatchStackEntry> {
    stack.iter().filter(|entry| !entry.touched_files.is_disjoint(relevant_files)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_skips_irrelevant_revisions() {
        let history = (0..5).map(|index| RevisionCandidate { revision: format!("r{index}"), parent: index.checked_sub(1).map(|parent| format!("r{parent}")), changed_files: BTreeSet::from([if index == 3 { "Hero.tsx".into() } else { "README.md".into() }]), environment_hash: "env".into() }).collect::<Vec<_>>();
        let contract = BisectContract { target: "hero".into(), good_revision: "r0".into(), bad_revision: "r4".into(), environment_hash: "env".into(), relevant_files: BTreeSet::from(["Hero.tsx".into()]), allow_working_tree: false };
        assert_eq!(plan(&history, &contract, &BTreeMap::new()).expect("step").revision, "r3");
    }
}