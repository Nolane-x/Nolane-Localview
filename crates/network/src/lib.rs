#![forbid(unsafe_code)]
use std::collections::HashMap;
use serde::{Deserialize,Serialize};

#[derive(Debug,Clone,Serialize,Deserialize)] pub struct RequestRecord{pub id:String,pub method:String,pub url:String,pub status:Option<u16>,pub duration_ms:u64,pub encoded_bytes:Option<u64>,pub from_cache:bool,pub error:Option<String>,pub initiator:Option<String>}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub enum NetworkIssueKind{Failed,Slow,Duplicate,LargePayload,Cors,MixedContent}
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct NetworkFinding{pub kind:NetworkIssueKind,pub request_ids:Vec<String>,pub message:String,pub confidence:u8}
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct NetworkPolicy{pub slow_ms:u64,pub large_bytes:u64}
impl Default for NetworkPolicy{fn default()->Self{Self{slow_ms:1_500,large_bytes:1_500_000}}}
pub fn analyze(records:&[RequestRecord],policy:&NetworkPolicy)->Vec<NetworkFinding>{let mut out=Vec::new();let mut groups:HashMap<(String,String),Vec<&RequestRecord>>=HashMap::new();for r in records{groups.entry((r.method.clone(),r.url.clone())).or_default().push(r);if r.error.is_some()||r.status.is_some_and(|s|s>=400){out.push(NetworkFinding{kind:NetworkIssueKind::Failed,request_ids:vec![r.id.clone()],message:format!("{} {} failed ({:?})",r.method,r.url,r.status),confidence:100});}if r.duration_ms>=policy.slow_ms{out.push(NetworkFinding{kind:NetworkIssueKind::Slow,request_ids:vec![r.id.clone()],message:format!("{}ms request",r.duration_ms),confidence:100});}if r.encoded_bytes.is_some_and(|b|b>=policy.large_bytes){out.push(NetworkFinding{kind:NetworkIssueKind::LargePayload,request_ids:vec![r.id.clone()],message:format!("large payload: {} bytes",r.encoded_bytes.unwrap()),confidence:100});}if r.error.as_deref().is_some_and(|e|e.to_ascii_lowercase().contains("cors")){out.push(NetworkFinding{kind:NetworkIssueKind::Cors,request_ids:vec![r.id.clone()],message:"CORS failure".into(),confidence:95});}}
for ((_method,_url),items) in groups{if items.len()>=3{out.push(NetworkFinding{kind:NetworkIssueKind::Duplicate,request_ids:items.iter().map(|r|r.id.clone()).collect(),message:format!("{} equivalent requests",items.len()),confidence:82});}}out}
#[cfg(test)]mod tests{use super::*;#[test]fn groups_duplicates(){let r=(0..3).map(|i|RequestRecord{id:i.to_string(),method:"GET".into(),url:"http://x/api".into(),status:Some(200),duration_ms:5,encoded_bytes:None,from_cache:false,error:None,initiator:None}).collect::<Vec<_>>();assert!(analyze(&r,&Default::default()).iter().any(|x|x.kind==NetworkIssueKind::Duplicate));}}


use std::collections::BTreeSet;

const MAX_FAULT_RULES: usize = 16;
const MAX_FAULT_PATH_BYTES: usize = 256;
const MIN_FAULT_LEASE_MS: u64 = 100;
const MAX_FAULT_LEASE_MS: u64 = 30_000;
const MAX_FAULT_DELAY_MS: u64 = 5_000;
const MAX_FAULT_HITS: u16 = 64;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum FaultTransport {
    Fetch,
    Xhr,
    Both,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum FaultMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkFaultEffect {
    Fail,
    Delay { milliseconds: u64 },
    MockStatus { status: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NetworkFaultRule {
    pub id: String,
    pub transport: FaultTransport,
    pub method: FaultMethod,
    pub path: String,
    pub effect: NetworkFaultEffect,
    pub max_hits: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NetworkFaultPlan {
    pub rules: Vec<NetworkFaultRule>,
    pub lease_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalNetworkFaultPlan {
    pub rules: Vec<NetworkFaultRule>,
    pub lease_ms: u64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkFaultPolicyError {
    EmptyRules,
    TooManyRules {
        count: usize,
        max: usize,
    },
    InvalidRuleId {
        id: String,
    },
    InvalidPath {
        path: String,
    },
    DuplicateSelector {
        transport: FaultTransport,
        method: FaultMethod,
        path: String,
    },
    InvalidLease {
        milliseconds: u64,
    },
    InvalidDelay {
        milliseconds: u64,
    },
    InvalidStatus {
        status: u16,
    },
    InvalidHitBudget {
        max_hits: u16,
    },
}

impl std::fmt::Display for NetworkFaultPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for NetworkFaultPolicyError {}

fn valid_fault_rule_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_fault_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_FAULT_PATH_BYTES
        && path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('?')
        && !path.contains('#')
        && !path.chars().any(char::is_control)
}

fn effect_tag(effect: &NetworkFaultEffect) -> (&'static str, u64) {
    match effect {
        NetworkFaultEffect::Fail => ("fail", 0),
        NetworkFaultEffect::Delay { milliseconds } => ("delay", *milliseconds),
        NetworkFaultEffect::MockStatus { status } => ("mock_status", u64::from(*status)),
    }
}

fn method_tag(method: FaultMethod) -> &'static str {
    match method {
        FaultMethod::Get => "get",
        FaultMethod::Head => "head",
        FaultMethod::Post => "post",
        FaultMethod::Put => "put",
        FaultMethod::Patch => "patch",
        FaultMethod::Delete => "delete",
        FaultMethod::Options => "options",
    }
}

fn transport_tag(transport: FaultTransport) -> &'static str {
    match transport {
        FaultTransport::Fetch => "fetch",
        FaultTransport::Xhr => "xhr",
        FaultTransport::Both => "both",
    }
}

fn feed_fingerprint(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

fn fault_plan_fingerprint(plan: &NetworkFaultPlan) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    feed_fingerprint(&mut hash, &plan.lease_ms.to_le_bytes());
    feed_fingerprint(&mut hash, &(plan.rules.len() as u64).to_le_bytes());

    for rule in &plan.rules {
        feed_fingerprint(&mut hash, rule.id.as_bytes());
        feed_fingerprint(&mut hash, transport_tag(rule.transport).as_bytes());
        feed_fingerprint(&mut hash, method_tag(rule.method).as_bytes());
        feed_fingerprint(&mut hash, rule.path.as_bytes());
        let (effect, value) = effect_tag(&rule.effect);
        feed_fingerprint(&mut hash, effect.as_bytes());
        feed_fingerprint(&mut hash, &value.to_le_bytes());
        feed_fingerprint(&mut hash, &rule.max_hits.to_le_bytes());
    }

    format!("{hash:016x}")
}

pub fn canonicalize_fault_plan(
    plan: &NetworkFaultPlan,
) -> Result<CanonicalNetworkFaultPlan, NetworkFaultPolicyError> {
    if plan.rules.is_empty() {
        return Err(NetworkFaultPolicyError::EmptyRules);
    }
    if plan.rules.len() > MAX_FAULT_RULES {
        return Err(NetworkFaultPolicyError::TooManyRules {
            count: plan.rules.len(),
            max: MAX_FAULT_RULES,
        });
    }
    if !(MIN_FAULT_LEASE_MS..=MAX_FAULT_LEASE_MS).contains(&plan.lease_ms) {
        return Err(NetworkFaultPolicyError::InvalidLease {
            milliseconds: plan.lease_ms,
        });
    }

    let mut selectors = BTreeSet::new();
    for rule in &plan.rules {
        if !valid_fault_rule_id(&rule.id) {
            return Err(NetworkFaultPolicyError::InvalidRuleId {
                id: rule.id.clone(),
            });
        }
        if !valid_fault_path(&rule.path) {
            return Err(NetworkFaultPolicyError::InvalidPath {
                path: rule.path.clone(),
            });
        }
        if !(1..=MAX_FAULT_HITS).contains(&rule.max_hits) {
            return Err(NetworkFaultPolicyError::InvalidHitBudget {
                max_hits: rule.max_hits,
            });
        }

        match rule.effect {
            NetworkFaultEffect::Fail => {}
            NetworkFaultEffect::Delay { milliseconds }
                if !(1..=MAX_FAULT_DELAY_MS).contains(&milliseconds) =>
            {
                return Err(NetworkFaultPolicyError::InvalidDelay { milliseconds });
            }
            NetworkFaultEffect::Delay { .. } => {}
            NetworkFaultEffect::MockStatus { status } if !(200..=599).contains(&status) => {
                return Err(NetworkFaultPolicyError::InvalidStatus { status });
            }
            NetworkFaultEffect::MockStatus { .. } => {}
        }

        let selector = (rule.transport, rule.method, rule.path.clone());
        if !selectors.insert(selector.clone()) {
            return Err(NetworkFaultPolicyError::DuplicateSelector {
                transport: selector.0,
                method: selector.1,
                path: selector.2,
            });
        }
    }

    Ok(CanonicalNetworkFaultPlan {
        rules: plan.rules.clone(),
        lease_ms: plan.lease_ms,
        fingerprint: fault_plan_fingerprint(plan),
    })
}
