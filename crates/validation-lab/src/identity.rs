use serde::{Deserialize, Serialize};

use crate::{CanonicalDigest, LabError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabRevisionContext {
    pub lab_revision: String,
    pub seed_corpus_revision: String,
    pub spec_revision_digest: String,
    pub reference_reducer_revision: String,
    pub mutation_catalog_revision: String,
    pub comparison_profile_revision: String,
    pub random_source_profile: String,
    pub platform_profile: Option<String>,
    pub start_sequence: u64,
}

impl LabRevisionContext {
    pub(crate) fn validate(&self) -> Result<(), LabError> {
        require("lab_revision", &self.lab_revision)?;
        require("seed_corpus_revision", &self.seed_corpus_revision)?;
        require("spec_revision_digest", &self.spec_revision_digest)?;
        require("reference_reducer_revision", &self.reference_reducer_revision)?;
        require("mutation_catalog_revision", &self.mutation_catalog_revision)?;
        require("comparison_profile_revision", &self.comparison_profile_revision)?;
        require("random_source_profile", &self.random_source_profile)?;
        if let Some(platform_profile) = &self.platform_profile {
            require("platform_profile", platform_profile)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedLabRunIdentity {
    pub revision_context: LabRevisionContext,
    pub result_artifact_digest: CanonicalDigest,
}

fn require(field: &'static str, value: &str) -> Result<(), LabError> {
    if value.is_empty() {
        Err(LabError::EmptyAuthorityField { field })
    } else {
        Ok(())
    }
}
