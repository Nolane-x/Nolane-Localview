use std::collections::{BTreeMap, BTreeSet};

use localview_evidence::{EvidenceId, EvidenceObject, UncertaintyClass};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ContractCompileError, ContractConflict, ContractException, ContractPredicate, ContractRegistry,
    ContractResult, ContractStrength, ContractVerdict, RuntimeFacts, UxContract, evaluate,
    predicates_conflict,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFactDomain {
    Selectors,
    Metrics,
    Values,
    IssueCodes,
    InteractiveNames,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LiveRuntimeFacts {
    pub facts: RuntimeFacts,
    pub complete_domains: BTreeSet<RuntimeFactDomain>,
    pub metric_keys: BTreeSet<String>,
    pub value_keys: BTreeSet<String>,
    pub evidence_ids: Vec<EvidenceId>,
    pub stale_evidence_ids: Vec<EvidenceId>,
    pub ignored_tainted_evidence_ids: Vec<EvidenceId>,
    pub ignored_non_authoritative_evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AutonomousContractCompileError {
    Contract(ContractCompileError),
    Conflict(Vec<ContractConflict>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompiledContractSet {
    pub contracts: Vec<UxContract>,
    pub contract_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractEvaluationRecord {
    pub contract_id: String,
    pub strength: ContractStrength,
    pub verdict: ContractVerdict,
    pub explanation: String,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContractEvaluationSummary {
    pub evaluated: Vec<ContractEvaluationRecord>,
    pub hard_failures: Vec<String>,
    pub hard_unknowns: Vec<String>,
    pub soft_warnings: Vec<String>,
    pub pass_count: usize,
    pub excepted_count: usize,
}

impl ContractRegistry {
    pub fn compile_autonomous_subset(
        &self,
        requested_ids: &BTreeSet<String>,
    ) -> Result<CompiledContractSet, AutonomousContractCompileError> {
        let ids = if requested_ids.is_empty() {
            self.contracts.keys().cloned().collect::<Vec<_>>()
        } else {
            requested_ids.iter().cloned().collect::<Vec<_>>()
        };

        let mut contracts = Vec::with_capacity(ids.len());
        for id in &ids {
            contracts.push(
                self.effective_contract(id)
                    .map_err(AutonomousContractCompileError::Contract)?,
            );
        }

        let mut conflicts = Vec::new();
        for (index, left) in contracts.iter().enumerate() {
            for right in contracts.iter().skip(index + 1) {
                if left.scope == right.scope
                    && predicates_conflict(&left.predicate, &right.predicate)
                {
                    conflicts.push(ContractConflict {
                        left: left.id.clone(),
                        right: right.id.clone(),
                        reason: "effective scope contains incompatible predicates".into(),
                    });
                }
            }
        }
        if !conflicts.is_empty() {
            return Err(AutonomousContractCompileError::Conflict(conflicts));
        }

        Ok(CompiledContractSet {
            contract_ids: contracts.iter().map(|contract| contract.id.clone()).collect(),
            contracts,
        })
    }
}

pub fn compile_runtime_facts(evidence: &[EvidenceObject], revision: &str) -> LiveRuntimeFacts {
    let mut output = LiveRuntimeFacts::default();
    let mut evidence_ids = BTreeSet::new();
    let mut stale = BTreeSet::new();
    let mut tainted = BTreeSet::new();
    let mut non_authoritative = BTreeSet::new();

    for item in evidence {
        if item.secret_taint {
            tainted.insert(item.id.clone());
            continue;
        }
        if item.provenance.revision.as_deref() != Some(revision) {
            stale.insert(item.id.clone());
            continue;
        }
        if !matches!(
            item.uncertainty,
            UncertaintyClass::Observed | UncertaintyClass::Derived
        ) {
            non_authoritative.insert(item.id.clone());
            continue;
        }
        let Some(object) = item.payload.as_object() else {
            continue;
        };

        let before = fact_weight(&output);
        compile_fact_payload(object, &mut output);
        if fact_weight(&output) > before {
            evidence_ids.insert(item.id.clone());
        }
    }

    output.evidence_ids = evidence_ids.into_iter().collect();
    output.stale_evidence_ids = stale.into_iter().collect();
    output.ignored_tainted_evidence_ids = tainted.into_iter().collect();
    output.ignored_non_authoritative_evidence_ids = non_authoritative.into_iter().collect();
    output
}

fn fact_weight(facts: &LiveRuntimeFacts) -> usize {
    facts.facts.selectors.len()
        + facts.facts.metrics.len()
        + facts.facts.values.len()
        + facts.facts.issue_codes.len()
        + facts.metric_keys.len()
        + facts.value_keys.len()
        + facts.complete_domains.len()
        + usize::from(facts.facts.unnamed_interactive_count > 0)
}

fn compile_fact_payload(
    object: &serde_json::Map<String, Value>,
    output: &mut LiveRuntimeFacts,
) {
    if let Some(selectors) = object.get("selectors").and_then(Value::as_array) {
        output.facts.selectors.extend(
            selectors
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
        );
    }
    if object
        .get("selectors_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output.complete_domains.insert(RuntimeFactDomain::Selectors);
    }

    if let Some(metrics) = object.get("metrics").and_then(Value::as_object) {
        for (key, value) in metrics {
            if let Some(number) = value.as_f64().filter(|number| number.is_finite()) {
                output.facts.metrics.insert(key.clone(), number);
                output.metric_keys.insert(key.clone());
            }
        }
    }
    if object
        .get("metrics_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output.complete_domains.insert(RuntimeFactDomain::Metrics);
    }

    if let Some(values) = object.get("values").and_then(Value::as_object) {
        for (key, value) in values {
            output.facts.values.insert(key.clone(), value.clone());
            output.value_keys.insert(key.clone());
        }
    }
    if object
        .get("values_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output.complete_domains.insert(RuntimeFactDomain::Values);
    }

    if let Some(issue_codes) = object.get("issue_codes").and_then(Value::as_array) {
        output.facts.issue_codes.extend(
            issue_codes
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned),
        );
    }
    if object
        .get("issue_codes_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output.complete_domains.insert(RuntimeFactDomain::IssueCodes);
    }

    if let Some(count) = object
        .get("unnamed_interactive_count")
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
    {
        output.facts.unnamed_interactive_count =
            output.facts.unnamed_interactive_count.max(count);
    }
    if object
        .get("interactive_names_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        output
            .complete_domains
            .insert(RuntimeFactDomain::InteractiveNames);
    }
}

pub fn evaluate_live_contract(
    contract: &UxContract,
    live: &LiveRuntimeFacts,
    exception: Option<&ContractException>,
    revision: &str,
) -> ContractResult {
    let exception = exception.filter(|exception| {
        exception.contract_id == contract.id
            && exception
                .expires_revision
                .as_deref()
                .is_none_or(|bound| bound == revision)
    });
    if let Some(result) = decisive_partial_result(contract, live, exception) {
        return result;
    }

    let domain_known = match &contract.predicate {
        ContractPredicate::Exists { .. } | ContractPredicate::NotExists { .. } => {
            live.complete_domains.contains(&RuntimeFactDomain::Selectors)
        }
        ContractPredicate::MetricAtMost { metric, .. }
        | ContractPredicate::MetricAtLeast { metric, .. } => {
            live.metric_keys.contains(metric)
                || live.complete_domains.contains(&RuntimeFactDomain::Metrics)
        }
        ContractPredicate::Equals { key, .. } => {
            live.value_keys.contains(key)
                || live.complete_domains.contains(&RuntimeFactDomain::Values)
        }
        ContractPredicate::NoIssueCode { .. } => {
            live.complete_domains.contains(&RuntimeFactDomain::IssueCodes)
        }
        ContractPredicate::EveryInteractiveNamed => live
            .complete_domains
            .contains(&RuntimeFactDomain::InteractiveNames),
    };

    if !domain_known {
        return ContractResult {
            contract_id: contract.id.clone(),
            verdict: ContractVerdict::Unknown,
            explanation: "required runtime fact domain is incomplete".into(),
            evidence_ids: live.evidence_ids.clone(),
        };
    }

    evaluate(
        contract,
        &live.facts,
        exception,
        live.evidence_ids.clone(),
    )
}

fn decisive_partial_result(
    contract: &UxContract,
    live: &LiveRuntimeFacts,
    exception: Option<&ContractException>,
) -> Option<ContractResult> {
    if exception.is_some() {
        return Some(evaluate(
            contract,
            &live.facts,
            exception,
            live.evidence_ids.clone(),
        ));
    }

    let fail = match &contract.predicate {
        ContractPredicate::Exists { selector } if live.facts.selectors.contains(selector) => {
            return Some(ContractResult {
                contract_id: contract.id.clone(),
                verdict: ContractVerdict::Pass,
                explanation: format!("selector {selector} exists in fresh evidence"),
                evidence_ids: live.evidence_ids.clone(),
            });
        }
        ContractPredicate::NotExists { selector } if live.facts.selectors.contains(selector) => {
            Some(format!("selector {selector} unexpectedly exists"))
        }
        ContractPredicate::NoIssueCode { code } if live.facts.issue_codes.contains(code) => {
            Some(format!("issue {code} present"))
        }
        ContractPredicate::EveryInteractiveNamed
            if live.facts.unnamed_interactive_count > 0 =>
        {
            Some(format!(
                "{} interactive element(s) are unnamed",
                live.facts.unnamed_interactive_count
            ))
        }
        _ => None,
    };

    fail.map(|explanation| ContractResult {
        contract_id: contract.id.clone(),
        verdict: ContractVerdict::Fail,
        explanation,
        evidence_ids: live.evidence_ids.clone(),
    })
}

pub fn evaluate_compiled_contracts(
    compiled: &CompiledContractSet,
    live: &LiveRuntimeFacts,
    exceptions: &BTreeMap<String, ContractException>,
    revision: &str,
) -> ContractEvaluationSummary {
    let mut summary = ContractEvaluationSummary::default();
    for contract in &compiled.contracts {
        let result = evaluate_live_contract(
            contract,
            live,
            exceptions.get(&contract.id),
            revision,
        );
        let record = ContractEvaluationRecord {
            contract_id: contract.id.clone(),
            strength: contract.strength,
            verdict: result.verdict,
            explanation: result.explanation,
            evidence_ids: result.evidence_ids,
        };
        match (record.strength, record.verdict) {
            (_, ContractVerdict::Pass) => summary.pass_count += 1,
            (_, ContractVerdict::Excepted) => summary.excepted_count += 1,
            (ContractStrength::Hard, ContractVerdict::Fail) => {
                summary.hard_failures.push(record.contract_id.clone())
            }
            (ContractStrength::Hard, ContractVerdict::Unknown) => {
                summary.hard_unknowns.push(record.contract_id.clone())
            }
            (ContractStrength::Soft, ContractVerdict::Fail | ContractVerdict::Unknown) => {
                summary.soft_warnings.push(record.contract_id.clone())
            }
        }
        summary.evaluated.push(record);
    }
    summary
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use localview_evidence::{EvidenceKind, Provenance};
    use localview_protocol::SessionId;
    use serde_json::json;

    use super::*;
    use crate::{ContractCategory, ContractScope};

    fn contract(id: &str, strength: ContractStrength, predicate: ContractPredicate) -> UxContract {
        UxContract {
            id: id.into(),
            title: id.into(),
            category: ContractCategory::Safety,
            strength,
            scope: ContractScope::global(),
            predicate,
            provenance: "test".into(),
            inherited_from: None,
        }
    }

    fn evidence(payload: Value, revision: &str) -> EvidenceObject {
        EvidenceObject {
            id: format!("ev-{revision}"),
            kind: EvidenceKind::Contract,
            session_id: SessionId::new_v4(),
            region: None,
            payload,
            provenance: Provenance {
                source: "wave9-test".into(),
                engine: None,
                revision: Some(revision.into()),
                parent_ids: vec![],
                captured_at: Utc::now(),
            },
            confidence: 1.0,
            uncertainty: UncertaintyClass::Observed,
            secret_taint: false,
        }
    }

    #[test]
    fn absent_selector_is_unknown_until_selector_denominator_is_complete() {
        let c = contract(
            "exists.hero",
            ContractStrength::Hard,
            ContractPredicate::Exists {
                selector: "#hero".into(),
            },
        );
        let live = compile_runtime_facts(&[evidence(json!({"selectors":[]}), "abc")], "abc");
        assert_eq!(
            evaluate_live_contract(&c, &live, None, "abc").verdict,
            ContractVerdict::Unknown
        );
    }

    #[test]
    fn observed_forbidden_issue_fails_even_when_issue_denominator_is_incomplete() {
        let c = contract(
            "no.crash",
            ContractStrength::Hard,
            ContractPredicate::NoIssueCode {
                code: "crash".into(),
            },
        );
        let live =
            compile_runtime_facts(&[evidence(json!({"issue_codes":["crash"]}), "abc")], "abc");
        assert_eq!(
            evaluate_live_contract(&c, &live, None, "abc").verdict,
            ContractVerdict::Fail
        );
    }

    #[test]
    fn hard_unknown_and_soft_failure_do_not_masquerade_as_passes() {
        let compiled = CompiledContractSet {
            contract_ids: vec!["hard".into(), "soft".into()],
            contracts: vec![
                contract(
                    "hard",
                    ContractStrength::Hard,
                    ContractPredicate::MetricAtMost {
                        metric: "lcp".into(),
                        max: 100.0,
                    },
                ),
                contract(
                    "soft",
                    ContractStrength::Soft,
                    ContractPredicate::NoIssueCode {
                        code: "style".into(),
                    },
                ),
            ],
        };
        let live = compile_runtime_facts(
            &[evidence(
                json!({"issue_codes":["style"],"issue_codes_complete":true}),
                "abc",
            )],
            "abc",
        );
        let summary =
            evaluate_compiled_contracts(&compiled, &live, &BTreeMap::new(), "abc");
        assert_eq!(summary.hard_unknowns, vec!["hard"]);
        assert_eq!(summary.soft_warnings, vec!["soft"]);
        assert_eq!(summary.pass_count, 0);
    }

    #[test]
    fn effective_conflict_and_inheritance_cycle_fail_compile() {
        let scope = ContractScope::global();
        let mut registry = ContractRegistry::default();
        registry.insert(UxContract {
            id: "parent".into(),
            title: "parent".into(),
            category: ContractCategory::Layout,
            strength: ContractStrength::Hard,
            scope: scope.clone(),
            predicate: ContractPredicate::Exists {
                selector: "#hero".into(),
            },
            provenance: "test".into(),
            inherited_from: None,
        });
        registry.insert(UxContract {
            id: "child".into(),
            title: "child".into(),
            category: ContractCategory::Layout,
            strength: ContractStrength::Hard,
            scope: ContractScope::global(),
            predicate: ContractPredicate::NotExists {
                selector: "#hero".into(),
            },
            provenance: "test".into(),
            inherited_from: Some("parent".into()),
        });
        assert!(matches!(
            registry.compile_autonomous_subset(&BTreeSet::new()),
            Err(AutonomousContractCompileError::Conflict(_))
        ));

        let mut cycle = ContractRegistry::default();
        let mut a = contract(
            "a",
            ContractStrength::Hard,
            ContractPredicate::Exists {
                selector: "#a".into(),
            },
        );
        a.inherited_from = Some("b".into());
        let mut b = contract(
            "b",
            ContractStrength::Hard,
            ContractPredicate::Exists {
                selector: "#b".into(),
            },
        );
        b.inherited_from = Some("a".into());
        cycle.insert(a);
        cycle.insert(b);
        assert!(matches!(
            cycle.compile_autonomous_subset(&BTreeSet::new()),
            Err(AutonomousContractCompileError::Contract(
                ContractCompileError::InheritanceCycle(_)
            ))
        ));
    }
}
