use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CanonicalDigest, LabError, LabMetricKind, LabRevisionContext, LabSeedIdentity,
    canonical_json_bytes,
    canonical::digest_canonical_bytes,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CampaignLayer {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
    L6,
    L7,
    L8,
    L9,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabPreregistration {
    pub revision_context: LabRevisionContext,
    pub seed_catalog_digest: CanonicalDigest,
    pub seed_identities: Vec<LabSeedIdentity>,
    pub campaign_layer: CampaignLayer,
    pub expected_distinctions: BTreeSet<String>,
    pub model_bound: Option<u64>,
    pub assumptions: BTreeSet<String>,
    pub declared_metrics: BTreeSet<LabMetricKind>,
    pub creation_sequence: u64,
}

impl LabPreregistration {
    pub fn prepare(&self) -> Result<PreparedPreregistration, LabError> {
        self.revision_context.validate()?;
        for identity in &self.seed_identities {
            identity.validate()?;
        }

        let canonical_bytes = canonical_json_bytes(self)?;
        let digest = digest_canonical_bytes(&canonical_bytes);
        Ok(PreparedPreregistration {
            canonical_bytes,
            digest,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedPreregistration {
    pub canonical_bytes: Vec<u8>,
    pub digest: CanonicalDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedPreregistrationReceipt {
    pub digest: CanonicalDigest,
    pub logical_sequence: u64,
    pub persistence_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedPreregistrationReceipt {
    digest: CanonicalDigest,
    logical_sequence: u64,
    persistence_ref: String,
}

impl ValidatedPreregistrationReceipt {
    pub fn digest(&self) -> &CanonicalDigest {
        &self.digest
    }

    pub fn logical_sequence(&self) -> u64 {
        self.logical_sequence
    }

    pub fn persistence_ref(&self) -> &str {
        &self.persistence_ref
    }
}

pub fn validate_persisted_receipt(
    prepared: &PreparedPreregistration,
    receipt: PersistedPreregistrationReceipt,
) -> Result<ValidatedPreregistrationReceipt, LabError> {
    if receipt.digest != prepared.digest {
        return Err(LabError::PreregistrationPersistenceMismatch {
            expected: prepared.digest.clone(),
            actual: receipt.digest,
        });
    }
    if receipt.logical_sequence == 0 {
        return Err(LabError::InvalidPersistenceReceipt {
            reason: "logical_sequence must be greater than zero",
        });
    }
    if receipt.persistence_ref.is_empty() {
        return Err(LabError::InvalidPersistenceReceipt {
            reason: "persistence_ref cannot be empty",
        });
    }

    Ok(ValidatedPreregistrationReceipt {
        digest: receipt.digest,
        logical_sequence: receipt.logical_sequence,
        persistence_ref: receipt.persistence_ref,
    })
}
