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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeObject {
    pub kind: RuntimeObjectKind,
    pub schema_version: u32,
    pub payload: Value,
    pub dependencies: Vec<ObjectHash>,
}

impl RuntimeObject {
    pub fn hash(&self) -> ObjectHash { object_hash(self) }
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

    pub fn get(&self, hash: &str) -> Option<&RuntimeObject> { self.objects.get(hash) }

    pub fn len(&self) -> usize { self.objects.len() }
    pub fn is_empty(&self) -> bool { self.objects.is_empty() }

    pub fn dependency_closure(&self, root: &str) -> BTreeSet<ObjectHash> {
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::from([root.to_owned()]);
        while let Some(hash) = queue.pop_front() {
            if !visited.insert(hash.clone()) { continue; }
            if let Some(object) = self.objects.get(&hash) {
                for dependency in &object.dependencies { queue.push_back(dependency.clone()); }
            }
        }
        visited
    }

    pub fn collect_garbage(&mut self, roots: &[ObjectHash]) -> usize {
        let keep = roots.iter().flat_map(|root| self.dependency_closure(root)).collect::<BTreeSet<_>>();
        let before = self.objects.len();
        self.objects.retain(|hash, _| keep.contains(hash));
        before.saturating_sub(self.objects.len())
    }

    pub fn validate_dependencies(&self, root: &str) -> Vec<ObjectHash> {
        let Some(object) = self.objects.get(root) else { return vec![root.to_owned()]; };
        object.dependencies.iter().filter(|dependency| !self.objects.contains_key(*dependency)).cloned().collect()
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
    pub fn root_hash(&self) -> ObjectHash { object_hash(self) }
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

    #[test]
    fn canonical_hash_ignores_map_insertion_order() {
        let first = serde_json::json!({"a": 1, "b": 2});
        let second = serde_json::json!({"b": 2, "a": 1});
        assert_eq!(object_hash(&first), object_hash(&second));
    }

    #[test]
    fn store_deduplicates_and_gc_preserves_dependencies() {
        let mut store = ContentStore::default();
        let leaf = store.insert(RuntimeObject { kind: RuntimeObjectKind::Semantic, schema_version: 1, payload: serde_json::json!({"node": "hero"}), dependencies: vec![] });
        let root = store.insert(RuntimeObject { kind: RuntimeObjectKind::Proof, schema_version: 1, payload: serde_json::json!({"verdict": "pass"}), dependencies: vec![leaf.clone()] });
        store.insert(RuntimeObject { kind: RuntimeObjectKind::Asset, schema_version: 1, payload: serde_json::json!({"unused": true}), dependencies: vec![] });
        assert_eq!(store.collect_garbage(std::slice::from_ref(&root)), 1);
        assert!(store.get(&leaf).is_some());
    }
}