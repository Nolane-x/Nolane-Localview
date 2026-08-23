#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap, HashSet};
use localview_protocol::{ElementRef, LayoutChange, PageSnapshot, Rect, SemanticNode, StateDiff};

pub fn flatten(root: &SemanticNode) -> HashMap<ElementRef, &SemanticNode> {
    fn walk<'a>(node: &'a SemanticNode, out: &mut HashMap<ElementRef, &'a SemanticNode>) {
        out.insert(node.reference.clone(), node);
        for child in &node.children { walk(child, out); }
    }
    let mut out = HashMap::new(); walk(root, &mut out); out
}

pub fn snapshot_diff(before: &PageSnapshot, after: &PageSnapshot) -> StateDiff {
    let old = flatten(&before.root); let new = flatten(&after.root);
    let old_keys = old.keys().cloned().collect::<HashSet<_>>();
    let new_keys = new.keys().cloned().collect::<HashSet<_>>();
    let removed_refs = old_keys.difference(&new_keys).cloned().collect::<Vec<_>>();
    let mut changed_refs = Vec::new(); let mut layout_changes = Vec::new();
    for reference in old_keys.intersection(&new_keys) {
        let a = old.get(reference).unwrap(); let b = new.get(reference).unwrap();
        if semantic_signature(a) != semantic_signature(b) { changed_refs.push(reference.clone()); }
        if a.rect != b.rect { layout_changes.push(LayoutChange { reference: reference.clone(), before: a.rect.clone(), after: b.rect.clone() }); }
    }
    changed_refs.extend(new_keys.difference(&old_keys).cloned());
    changed_refs.sort(); changed_refs.dedup();
    StateDiff {
        from_version: before.version,
        to_version: after.version,
        changed_refs,
        removed_refs,
        route_changed: before.route != after.route,
        layout_changes,
        console_delta: multiset_delta(&before.console_errors, &after.console_errors),
        network_delta: multiset_delta(&before.failed_requests, &after.failed_requests),
    }
}

fn semantic_signature(node: &SemanticNode) -> String {
    let attrs = node.attributes.iter().map(|(k,v)| format!("{k}={v}")).collect::<Vec<_>>().join(";");
    format!("{}|{:?}|{:?}|{}|{}", node.tag, node.role, node.name, node.interactive, attrs)
}
fn multiset_delta<T: Clone + PartialEq>(before: &[T], after: &[T]) -> Vec<T> {
    after.iter().filter(|v| !before.contains(v)).cloned().collect()
}

pub fn compact_snapshot(snapshot: &PageSnapshot, max_nodes: usize) -> serde_json::Value {
    fn compact(node:&SemanticNode, remaining:&mut usize)->Option<serde_json::Value>{
        if *remaining==0{return None;} *remaining-=1;
        let children=node.children.iter().filter_map(|c|compact(c,remaining)).collect::<Vec<_>>();
        Some(serde_json::json!({"ref":node.reference,"role":node.role,"name":node.name,"tag":node.tag,"interactive":node.interactive,"rect":node.rect,"children":children}))
    }
    let mut remaining=max_nodes.max(1);
    serde_json::json!({"version":snapshot.version,"route":snapshot.route,"viewport":snapshot.viewport,"tree":compact(&snapshot.root,&mut remaining),"console_errors":snapshot.console_errors,"failed_requests":snapshot.failed_requests})
}

pub fn stable_ref(role: Option<&str>, name: Option<&str>, tag:&str, ancestry:&[&str]) -> ElementRef {
    let mut hash:u64=0xcbf29ce484222325;
    for byte in ancestry.iter().copied().chain([role.unwrap_or(""),name.unwrap_or(""),tag]).flat_map(str::bytes) { hash^=byte as u64; hash=hash.wrapping_mul(0x100000001b3); }
    format!("@e{:x}",hash)
}

#[cfg(test)]
mod tests {
    use super::*; use chrono::Utc; use localview_protocol::{ConsoleIssue,NetworkIssue,SourceLocation};
    fn node(r:&str,x:f64)->SemanticNode{SemanticNode{reference:r.into(),role:Some("button".into()),name:Some("Save".into()),tag:"button".into(),rect:Some(Rect{x,y:0.0,width:100.0,height:40.0}),interactive:true,attributes:BTreeMap::new(),source:None,children:vec![]}}
    fn snap(v:u64,x:f64)->PageSnapshot{PageSnapshot{version:v,route:"/".into(),viewport:(1440,900),root:node("@save",x),console_errors:vec![],failed_requests:vec![],captured_at:Utc::now()}}
    #[test] fn geometry_change_becomes_layout_delta(){let d=snapshot_diff(&snap(1,0.0),&snap(2,7.0));assert_eq!(d.layout_changes.len(),1);assert!(d.changed_refs.is_empty());}
    #[test] fn refs_are_stable(){assert_eq!(stable_ref(Some("button"),Some("Save"),"button",&["main"]),stable_ref(Some("button"),Some("Save"),"button",&["main"]));}
}
