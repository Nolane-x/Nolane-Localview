use serde::{Deserialize, Serialize};

use crate::{CanonicalDigest, LabError, canonical::digest_canonical_bytes, canonical_json_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabArtifactKind {
    Preregistration,
    SeedCatalog,
    Results,
    MutationReport,
    DifferentialReport,
    CoverageReport,
    Environment,
    Counterexamples,
    MinimizedSeeds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabArtifactState {
    Present,
    NotApplicable,
    NotRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalArtifact {
    pub kind: LabArtifactKind,
    pub canonical_bytes: Vec<u8>,
    pub digest: CanonicalDigest,
}

impl CanonicalArtifact {
    pub fn preregistration<T: Serialize>(payload: &T) -> Result<Self, LabError> {
        Self::from_payload(LabArtifactKind::Preregistration, payload)
    }

    pub fn seed_catalog<T: Serialize>(payload: &T) -> Result<Self, LabError> {
        Self::from_payload(LabArtifactKind::SeedCatalog, payload)
    }

    pub fn results<T: Serialize>(payload: &T) -> Result<Self, LabError> {
        Self::from_payload(LabArtifactKind::Results, payload)
    }

    fn from_payload<T: Serialize>(kind: LabArtifactKind, payload: &T) -> Result<Self, LabError> {
        let canonical_bytes = canonical_json_bytes(payload)?;
        let digest = digest_canonical_bytes(&canonical_bytes);
        Ok(Self {
            kind,
            canonical_bytes,
            digest,
        })
    }
}
