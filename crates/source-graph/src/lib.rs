#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use localview_protocol::{ElementRef, SourceLocation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceKey {
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl From<&SourceLocation> for SourceKey {
    fn from(source: &SourceLocation) -> Self {
        Self {
            file: source.file.clone(),
            line: Some(source.line),
            column: source.column,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegionBinding {
    pub region_id: String,
    pub element_refs: BTreeSet<ElementRef>,
    pub source_keys: BTreeSet<SourceKey>,
    pub confidence_milli: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRegionIndex {
    pub regions: BTreeMap<String, RegionBinding>,
    pub source_to_regions: BTreeMap<String, BTreeSet<String>>,
    pub ref_to_region: BTreeMap<ElementRef, String>,
}

impl SourceRegionIndex {
    pub fn upsert(&mut self, binding: RegionBinding) {
        if let Some(previous) = self.regions.insert(binding.region_id.clone(), binding.clone()) {
            for source in previous.source_keys {
                self.remove_source_region(&source.file, &previous.region_id);
            }
            for reference in previous.element_refs {
                self.ref_to_region.remove(&reference);
            }
        }
        for source in &binding.source_keys {
            self.source_to_regions
                .entry(source.file.clone())
                .or_default()
                .insert(binding.region_id.clone());
        }
        for reference in &binding.element_refs {
            self.ref_to_region
                .insert(reference.clone(), binding.region_id.clone());
        }
    }

    fn remove_source_region(&mut self, file: &str, region: &str) {
        let remove_entry = if let Some(regions) = self.source_to_regions.get_mut(file) {
            regions.remove(region);
            regions.is_empty()
        } else {
            false
        };
        if remove_entry {
            self.source_to_regions.remove(file);
        }
    }

    pub fn regions_for_source(&self, file: &str) -> Vec<&RegionBinding> {
        self.source_to_regions
            .get(file)
            .into_iter()
            .flatten()
            .filter_map(|region| self.regions.get(region))
            .collect()
    }

    pub fn source_for_region(&self, region: &str) -> Vec<SourceKey> {
        self.regions
            .get(region)
            .map(|binding| binding.source_keys.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn region_for_ref(&self, reference: &str) -> Option<&str> {
        self.ref_to_region.get(reference).map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Import,
    CssToken,
    Typography,
    Asset,
    Data,
    Route,
    Runtime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyGraph {
    pub edges: Vec<DependencyEdge>,
}

impl DependencyGraph {
    pub fn add(&mut self, mut edge: DependencyEdge) {
        edge.evidence_ids.sort();
        edge.evidence_ids.dedup();
        if !self.edges.iter().any(|existing| {
            existing.from == edge.from && existing.to == edge.to && existing.kind == edge.kind
        }) {
            self.edges.push(edge);
        }
    }

    pub fn blast_radius(
        &self,
        roots: &BTreeSet<String>,
        kinds: &BTreeSet<DependencyKind>,
        max_depth: usize,
    ) -> BTreeSet<String> {
        let mut impacted = roots.clone();
        let mut queue = roots
            .iter()
            .cloned()
            .map(|root| (root, 0usize))
            .collect::<VecDeque<_>>();
        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for edge in self.edges.iter().filter(|edge| {
                edge.from == current && (kinds.is_empty() || kinds.contains(&edge.kind))
            }) {
                if impacted.insert(edge.to.clone()) {
                    queue.push_back((edge.to.clone(), depth + 1));
                }
            }
        }
        impacted
    }

    pub fn reverse_dependencies(&self, target: &str) -> BTreeSet<String> {
        self.edges
            .iter()
            .filter(|edge| edge.to == target)
            .map(|edge| edge.from.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeImpact {
    pub changed_sources: BTreeSet<String>,
    pub impacted_regions: BTreeSet<String>,
    pub impacted_dependencies: BTreeSet<String>,
}

pub fn change_impact(
    index: &SourceRegionIndex,
    graph: &DependencyGraph,
    changed_sources: BTreeSet<String>,
    max_depth: usize,
) -> ChangeImpact {
    let impacted_regions = changed_sources
        .iter()
        .flat_map(|file| index.regions_for_source(file))
        .map(|binding| binding.region_id.clone())
        .collect::<BTreeSet<_>>();
    let impacted_dependencies =
        graph.blast_radius(&changed_sources, &BTreeSet::new(), max_depth);
    ChangeImpact {
        changed_sources,
        impacted_regions,
        impacted_dependencies,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_change_maps_to_runtime_region() {
        let mut index = SourceRegionIndex::default();
        index.upsert(RegionBinding {
            region_id: "hero".into(),
            element_refs: BTreeSet::from(["heading".into()]),
            source_keys: BTreeSet::from([SourceKey {
                file: "Hero.tsx".into(),
                line: Some(10),
                column: Some(1),
            }]),
            confidence_milli: 950,
        });
        assert_eq!(index.regions_for_source("Hero.tsx")[0].region_id, "hero");
        assert_eq!(index.region_for_ref("heading"), Some("hero"));
    }

    #[test]
    fn token_dependency_blast_radius_is_transitive() {
        let mut graph = DependencyGraph::default();
        graph.add(DependencyEdge {
            from: "tokens.css".into(),
            to: "Button.tsx".into(),
            kind: DependencyKind::CssToken,
            evidence_ids: vec![],
        });
        graph.add(DependencyEdge {
            from: "Button.tsx".into(),
            to: "checkout".into(),
            kind: DependencyKind::Import,
            evidence_ids: vec![],
        });
        let impacted = graph.blast_radius(
            &BTreeSet::from(["tokens.css".into()]),
            &BTreeSet::new(),
            3,
        );
        assert!(impacted.contains("checkout"));
    }
}
