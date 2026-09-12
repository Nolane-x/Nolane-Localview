use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CanonicalDigest, LabError, canonical_digest};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LabSeedIdentity {
    pub seed_id: String,
    pub prediction_revision: String,
    pub oracle_revision: String,
}

impl LabSeedIdentity {
    pub(crate) fn validate(&self) -> Result<(), LabError> {
        require_authority_field("seed_id", &self.seed_id)?;
        require_authority_field("prediction_revision", &self.prediction_revision)?;
        require_authority_field("oracle_revision", &self.oracle_revision)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabSeed {
    pub identity: LabSeedIdentity,
    pub family: String,
    pub spec_surface_refs: BTreeSet<u32>,
    pub input_fixture: Value,
    pub expected_semantic_outcome: String,
    pub forbidden_outcomes: BTreeSet<String>,
    pub comparison_mode: String,
    pub risk_if_missed: String,
}

#[derive(Clone, Debug)]
pub struct LabSeedCatalog {
    corpus_revision: String,
    seeds: BTreeMap<LabSeedIdentity, LabSeed>,
}

#[derive(Serialize)]
struct CanonicalSeedCatalog<'a> {
    corpus_revision: &'a str,
    seeds: Vec<&'a LabSeed>,
}

impl LabSeedCatalog {
    pub fn new(
        corpus_revision: impl Into<String>,
        seeds: Vec<LabSeed>,
    ) -> Result<Self, LabError> {
        let corpus_revision = corpus_revision.into();
        require_authority_field("corpus_revision", &corpus_revision)?;

        let mut indexed = BTreeMap::new();
        for seed in seeds {
            seed.identity.validate()?;
            match indexed.entry(seed.identity.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(seed);
                }
                Entry::Occupied(entry) => {
                    let identity = entry.key();
                    return Err(LabError::DuplicateSeedIdentity {
                        seed_id: identity.seed_id.clone(),
                        prediction_revision: identity.prediction_revision.clone(),
                        oracle_revision: identity.oracle_revision.clone(),
                    });
                }
            }
        }

        Ok(Self {
            corpus_revision,
            seeds: indexed,
        })
    }

    pub fn corpus_revision(&self) -> &str {
        &self.corpus_revision
    }

    pub fn len(&self) -> usize {
        self.seeds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seeds.is_empty()
    }

    pub fn get(&self, identity: &LabSeedIdentity) -> Option<&LabSeed> {
        self.seeds.get(identity)
    }

    pub fn canonical_digest(&self) -> Result<CanonicalDigest, LabError> {
        canonical_digest(&CanonicalSeedCatalog {
            corpus_revision: &self.corpus_revision,
            seeds: self.seeds.values().collect(),
        })
    }
}

fn require_authority_field(field: &'static str, value: &str) -> Result<(), LabError> {
    if value.is_empty() {
        Err(LabError::EmptyAuthorityField { field })
    } else {
        Ok(())
    }
}
