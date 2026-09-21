#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub type ObjectHash = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeObjectKind {
    Semantic,
    Layout,
    Visual,
    Region,
    Evidence,
    Proof,
    Contract,
    State,
    Asset,
    Report,
    Baseline,
    Attestation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeObject {
    pub kind: RuntimeObjectKind,
    pub schema_version: u32,
    pub payload: Value,
    pub dependencies: Vec<ObjectHash>,
}

impl RuntimeObject {
    pub fn hash(&self) -> ObjectHash {
        object_hash(self)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContentStore {
    objects: HashMap<ObjectHash, RuntimeObject>,
}

impl ContentStore {
    pub fn insert(&mut self, mut object: RuntimeObject) -> ObjectHash {
        object.dependencies.sort();
        object.dependencies.dedup();
        let hash = object.hash();
        self.objects.entry(hash.clone()).or_insert(object);
        hash
    }

    pub fn get(&self, hash: &str) -> Option<&RuntimeObject> {
        self.objects.get(hash)
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn dependency_closure(&self, root: &str) -> BTreeSet<ObjectHash> {
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::from([root.to_owned()]);
        while let Some(hash) = queue.pop_front() {
            if !visited.insert(hash.clone()) {
                continue;
            }
            if let Some(object) = self.objects.get(&hash) {
                for dependency in &object.dependencies {
                    queue.push_back(dependency.clone());
                }
            }
        }
        visited
    }

    pub fn collect_garbage(&mut self, roots: &[ObjectHash]) -> usize {
        let keep = roots
            .iter()
            .flat_map(|root| self.dependency_closure(root))
            .collect::<BTreeSet<_>>();
        let before = self.objects.len();
        self.objects.retain(|hash, _| keep.contains(hash));
        before.saturating_sub(self.objects.len())
    }

    pub fn validate_dependencies(&self, root: &str) -> Vec<ObjectHash> {
        let Some(object) = self.objects.get(root) else {
            return vec![root.to_owned()];
        };
        object
            .dependencies
            .iter()
            .filter(|dependency| !self.objects.contains_key(*dependency))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BaselineEnvelope {
    pub schema_version: u32,
    pub state_identity: ObjectHash,
    pub route: String,
    pub viewport: (u32, u32),
    pub evidence_hashes: Vec<ObjectHash>,
    pub design_baseline_hash: Option<ObjectHash>,
    pub created_revision: Option<String>,
    pub provenance: BTreeMap<String, String>,
}

impl BaselineEnvelope {
    pub fn normalized(mut self) -> Self {
        self.evidence_hashes.sort();
        self.evidence_hashes.dedup();
        self
    }

    pub fn object(&self) -> RuntimeObject {
        let normalized = self.clone().normalized();
        let mut dependencies = normalized.evidence_hashes.clone();
        if let Some(design) = &normalized.design_baseline_hash {
            dependencies.push(design.clone());
        }
        dependencies.sort();
        dependencies.dedup();
        RuntimeObject {
            kind: RuntimeObjectKind::Baseline,
            schema_version: normalized.schema_version,
            payload: serde_json::to_value(&normalized).unwrap_or(Value::Null),
            dependencies,
        }
    }

    pub fn canonical_hash(&self) -> ObjectHash {
        self.object().hash()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegionMerkleNode {
    pub region_id: String,
    pub semantic_hash: Option<ObjectHash>,
    pub layout_hash: Option<ObjectHash>,
    pub visual_hash: Option<ObjectHash>,
    pub children: Vec<ObjectHash>,
}

impl RegionMerkleNode {
    pub fn root_hash(&self) -> ObjectHash {
        object_hash(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDelta {
    pub unchanged: Vec<ObjectHash>,
    pub added: Vec<ObjectHash>,
    pub removed: Vec<ObjectHash>,
}

pub fn diff_hash_sets(before: &[ObjectHash], after: &[ObjectHash]) -> RuntimeDelta {
    let before = before.iter().cloned().collect::<BTreeSet<_>>();
    let after = after.iter().cloned().collect::<BTreeSet<_>>();
    RuntimeDelta {
        unchanged: before.intersection(&after).cloned().collect(),
        added: after.difference(&before).cloned().collect(),
        removed: before.difference(&after).cloned().collect(),
    }
}

pub fn object_hash<T: Serialize>(value: &T) -> ObjectHash {
    let canonical = canonical_value(serde_json::to_value(value).unwrap_or(Value::Null));
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn canonical_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let ordered = map
                .into_iter()
                .map(|(key, value)| (key, canonical_value(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_value).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_hash_ignores_map_insertion_order() {
        let first = serde_json::json!({"a": 1, "b": 2});
        let second = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(object_hash(&first), object_hash(&second));
    }

    #[test]
    fn store_deduplicates_and_gc_preserves_dependencies() {
        let mut store = ContentStore::default();
        let leaf = store.insert(RuntimeObject {
            kind: RuntimeObjectKind::Semantic,
            schema_version: 1,
            payload: serde_json::json!({"node": "hero"}),
            dependencies: vec![],
        });
        let root = store.insert(RuntimeObject {
            kind: RuntimeObjectKind::Proof,
            schema_version: 1,
            payload: serde_json::json!({"verdict": "pass"}),
            dependencies: vec![leaf.clone()],
        });
        store.insert(RuntimeObject {
            kind: RuntimeObjectKind::Asset,
            schema_version: 1,
            payload: serde_json::json!({"unused": true}),
            dependencies: vec![],
        });
        assert_eq!(store.collect_garbage(std::slice::from_ref(&root)), 1);
        assert!(store.get(&leaf).is_some());
    }

    #[test]
    fn baseline_hash_is_reproducible_and_dependency_closure_is_exact() {
        let evidence = RuntimeObject {
            kind: RuntimeObjectKind::Evidence,
            schema_version: 1,
            payload: serde_json::json!({"id": "ev_1"}),
            dependencies: vec![],
        };
        let mut store = ContentStore::default();
        let evidence_hash = store.insert(evidence);
        let envelope = BaselineEnvelope {
            schema_version: 1,
            state_identity: object_hash(&serde_json::json!({"route": "/"})),
            route: "/".into(),
            viewport: (1280, 720),
            evidence_hashes: vec![evidence_hash.clone(), evidence_hash.clone()],
            design_baseline_hash: None,
            created_revision: Some("abc".into()),
            provenance: BTreeMap::from([("source".into(), "wave8-headless".into())]),
        }
        .normalized();
        let first = envelope.canonical_hash();
        let second = envelope.clone().canonical_hash();
        assert_eq!(first, second);
        let root = store.insert(envelope.object());
        assert_eq!(
            store.dependency_closure(&root),
            BTreeSet::from([root, evidence_hash])
        );
    }
}
