#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptVerdict { Pass, Fail, Inconclusive }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttestorKind { Local, Ci }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofReceiptPayload {
    pub schema_version: u32,
    pub project: String,
    pub baseline_revision: String,
    pub candidate_revision: String,
    pub environment_hash: String,
    pub plan_hash: String,
    pub evidence_hashes: Vec<String>,
    pub contract_hashes: Vec<String>,
    pub mutation_run_hash: Option<String>,
    pub verdict: ReceiptVerdict,
    pub created_at: DateTime<Utc>,
}

impl ProofReceiptPayload {
    pub fn canonical_hash(&self) -> String {
        let canonical = canonical_value(serde_json::to_value(self).unwrap_or(Value::Null));
        let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
        format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofReceipt {
    pub payload: ProofReceiptPayload,
    pub payload_hash: String,
    pub attestor: AttestorKind,
    pub key_id: String,
    pub signature: String,
}

pub fn sign(payload: ProofReceiptPayload, attestor: AttestorKind, key_id: impl Into<String>, key: &[u8]) -> Result<ProofReceipt, AttestationError> {
    if key.len() < 16 { return Err(AttestationError::WeakKey); }
    let payload_hash = payload.canonical_hash();
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| AttestationError::InvalidKey)?;
    mac.update(payload_hash.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    Ok(ProofReceipt { payload, payload_hash, attestor, key_id: key_id.into(), signature })
}

pub fn verify(receipt: &ProofReceipt, key: &[u8]) -> Result<bool, AttestationError> {
    if receipt.payload.canonical_hash() != receipt.payload_hash { return Ok(false); }
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| AttestationError::InvalidKey)?;
    mac.update(receipt.payload_hash.as_bytes());
    let signature = hex::decode(&receipt.signature).map_err(|_| AttestationError::InvalidSignatureEncoding)?;
    Ok(mac.verify_slice(&signature).is_ok())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AttestationError { WeakKey, InvalidKey, InvalidSignatureEncoding }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurrentRuntimeBinding {
    pub candidate_revision: String,
    pub environment_hash: String,
    pub plan_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StalenessReport { pub stale: bool, pub reasons: Vec<String> }

pub fn staleness(receipt: &ProofReceipt, current: &CurrentRuntimeBinding) -> StalenessReport {
    let mut reasons = Vec::new();
    if receipt.payload.candidate_revision != current.candidate_revision { reasons.push("candidate revision changed".into()); }
    if receipt.payload.environment_hash != current.environment_hash { reasons.push("environment changed".into()); }
    if receipt.payload.plan_hash != current.plan_hash { reasons.push("verification plan changed".into()); }
    StalenessReport { stale: !reasons.is_empty(), reasons }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrEvidenceReceipt {
    pub pull_request: String,
    pub head_revision: String,
    pub receipt_hashes: Vec<String>,
    pub all_pass: bool,
}

pub fn aggregate_pr(pull_request: impl Into<String>, head_revision: impl Into<String>, receipts: &[ProofReceipt]) -> PrEvidenceReceipt {
    PrEvidenceReceipt {
        pull_request: pull_request.into(),
        head_revision: head_revision.into(),
        receipt_hashes: receipts.iter().map(|receipt| receipt.payload_hash.clone()).collect(),
        all_pass: !receipts.is_empty() && receipts.iter().all(|receipt| receipt.payload.verdict == ReceiptVerdict::Pass),
    }
}

fn canonical_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let ordered = map.into_iter().map(|(key, value)| (key, canonical_value(value))).collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_value).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> ProofReceiptPayload {
        ProofReceiptPayload { schema_version: 1, project: "LocalView".into(), baseline_revision: "a".into(), candidate_revision: "b".into(), environment_hash: "env".into(), plan_hash: "plan".into(), evidence_hashes: vec!["ev".into()], contract_hashes: vec![], mutation_run_hash: None, verdict: ReceiptVerdict::Pass, created_at: DateTime::<Utc>::from_timestamp(1, 0).expect("timestamp") }
    }

    #[test]
    fn receipt_signature_detects_tampering() {
        let key = b"0123456789abcdef0123456789abcdef";
        let mut receipt = sign(payload(), AttestorKind::Local, "local", key).expect("sign");
        assert!(verify(&receipt, key).expect("verify"));
        receipt.payload.candidate_revision = "tampered".into();
        assert!(!verify(&receipt, key).expect("verify"));
    }
}