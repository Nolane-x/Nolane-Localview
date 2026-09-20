use std::{
    collections::{BTreeSet, HashMap},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde_json::Value;
use tokio::{
    fs,
    time::{Instant, timeout},
};

const MAX_SNAPSHOT_NODES: usize = 600;
const MAX_TREE_DEPTH: usize = 12;
const MAX_ABSOLUTE_VUE_PATH_BYTES: usize = 1_024;
const MAX_UNIQUE_ABSOLUTE_VUE_PATHS: usize = 64;
const MAX_RETAINED_VUE_PATH_BYTES: usize = 260;
const VUE_PATH_AUTHORITY_IO_BUDGET: Duration = Duration::from_millis(150);

pub(crate) async fn sanitize_vue_snapshot_paths(payload: &mut Value, root_hint: Option<&Path>) {
    let Some(raw_root) = payload.get("semantic_tree") else {
        return;
    };

    let mut remaining = MAX_SNAPSHOT_NODES;
    let mut candidates = BTreeSet::new();
    let mut needs_rewrite = false;
    if !collect_absolute_vue_paths(
        raw_root,
        0,
        &mut remaining,
        &mut candidates,
        &mut needs_rewrite,
    ) {
        if let Some(payload) = payload.as_object_mut() {
            payload.insert("semantic_tree".into(), Value::Null);
        }
        return;
    }
    if !needs_rewrite {
        return;
    }

    let deadline = Instant::now() + VUE_PATH_AUTHORITY_IO_BUDGET;
    let project_root = match root_hint {
        Some(root) => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                None
            } else {
                timeout(remaining, canonical_project_root(root))
                    .await
                    .ok()
                    .flatten()
            }
        }
        None => None,
    };

    let mut replacements = HashMap::new();
    for candidate in candidates {
        let replacement = match project_root.as_deref() {
            Some(root) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    None
                } else {
                    timeout(remaining, canonical_project_vue_file(root, &candidate))
                        .await
                        .ok()
                        .flatten()
                }
            }
            None => None,
        };
        replacements.insert(candidate, replacement);
    }

    let Some(raw_root) = payload.get_mut("semantic_tree") else {
        return;
    };
    let mut remaining = MAX_SNAPSHOT_NODES;
    rewrite_absolute_vue_paths(raw_root, 0, &mut remaining, &replacements);
}

async fn canonical_project_root(root: &Path) -> Option<PathBuf> {
    let root = fs::canonicalize(root).await.ok()?;
    fs::metadata(&root).await.ok()?.is_dir().then_some(root)
}

async fn canonical_project_vue_file(root: &Path, raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_ABSOLUTE_VUE_PATH_BYTES {
        return None;
    }
    if !is_absolute_vue_candidate(raw) {
        return None;
    }

    let candidate = fs::canonicalize(Path::new(raw)).await.ok()?;
    if !candidate.starts_with(root) || !fs::metadata(&candidate).await.ok()?.is_file() {
        return None;
    }
    if candidate.extension().and_then(|value| value.to_str()) != Some("vue") {
        return None;
    }

    project_relative_display(root, &candidate)
}

fn collect_absolute_vue_paths(
    node: &Value,
    depth: usize,
    remaining: &mut usize,
    output: &mut BTreeSet<String>,
    needs_rewrite: &mut bool,
) -> bool {
    if depth > MAX_TREE_DEPTH || *remaining == 0 {
        return false;
    }
    *remaining -= 1;

    match vue_hint_path(node) {
        VueHintPath::Absolute(file) => {
            *needs_rewrite = true;
            if output.len() < MAX_UNIQUE_ABSOLUTE_VUE_PATHS {
                output.insert(file);
            }
        }
        VueHintPath::Invalid => *needs_rewrite = true,
        VueHintPath::NotVue | VueHintPath::Relative => {}
    }

    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children {
            if !collect_absolute_vue_paths(child, depth + 1, remaining, output, needs_rewrite) {
                return false;
            }
        }
    }
    true
}

fn rewrite_absolute_vue_paths(
    node: &mut Value,
    depth: usize,
    remaining: &mut usize,
    replacements: &HashMap<String, Option<String>>,
) {
    if depth > MAX_TREE_DEPTH || *remaining == 0 {
        return;
    }
    *remaining -= 1;

    match vue_hint_path(node) {
        VueHintPath::Absolute(file) => match replacements.get(&file).and_then(Option::as_ref) {
            Some(relative) => {
                if let Some(component) = vue_component_for_file(relative) {
                    if let Some(source_hint) =
                        node.get_mut("sourceHint").and_then(Value::as_object_mut)
                    {
                        source_hint.insert("file".into(), Value::String(relative.clone()));
                        source_hint.insert("component".into(), Value::String(component));
                    }
                } else if let Some(node) = node.as_object_mut() {
                    node.insert("sourceHint".into(), Value::Null);
                }
            }
            None => {
                if let Some(node) = node.as_object_mut() {
                    node.insert("sourceHint".into(), Value::Null);
                }
            }
        },
        VueHintPath::Invalid => {
            if let Some(node) = node.as_object_mut() {
                node.insert("sourceHint".into(), Value::Null);
            }
        }
        VueHintPath::NotVue | VueHintPath::Relative => {}
    }

    if let Some(children) = node.get_mut("children").and_then(Value::as_array_mut) {
        for child in children {
            rewrite_absolute_vue_paths(child, depth + 1, remaining, replacements);
            if *remaining == 0 {
                break;
            }
        }
    }
}

enum VueHintPath {
    NotVue,
    Relative,
    Absolute(String),
    Invalid,
}

fn vue_hint_path(node: &Value) -> VueHintPath {
    let Some(hint) = node.get("sourceHint").and_then(Value::as_object) else {
        return VueHintPath::NotVue;
    };
    if hint.get("origin").and_then(Value::as_str) != Some("vue-dev-instance") {
        return VueHintPath::NotVue;
    }

    let Some(component) = hint.get("component").and_then(Value::as_str) else {
        return VueHintPath::Invalid;
    };
    let signal_valid =
        hint.get("signal").and_then(Value::as_str) == Some("element_parent_component");
    let Some(file) = hint.get("file").and_then(Value::as_str) else {
        return VueHintPath::Invalid;
    };
    let component_valid = vue_component_for_file(file)
        .as_deref()
        .is_some_and(|expected| expected == component);
    if !component_valid || !signal_valid || file.chars().any(char::is_control) {
        return VueHintPath::Invalid;
    }

    if is_safe_relative_vue_file(file) {
        return VueHintPath::Relative;
    }
    if is_absolute_vue_candidate(file)
        && file.len() <= MAX_ABSOLUTE_VUE_PATH_BYTES
        && file.ends_with(".vue")
    {
        return VueHintPath::Absolute(file.to_owned());
    }
    VueHintPath::Invalid
}

fn vue_component_for_file(file: &str) -> Option<String> {
    let normalized = file.replace('\\', "/");
    let basename = normalized.rsplit('/').next()?;
    let component = basename.strip_suffix(".vue")?;
    if component.is_empty() || component.len() > 96 || component.chars().any(char::is_control) {
        return None;
    }
    Some(component.to_owned())
}

fn is_safe_relative_vue_file(file: &str) -> bool {
    if file.is_empty()
        || file.len() > MAX_RETAINED_VUE_PATH_BYTES
        || !file.ends_with(".vue")
        || file.starts_with('/')
        || file.starts_with('\\')
        || file.contains('\\')
        || file
            .chars()
            .any(|character| matches!(character, '%' | '?' | '#' | ':') || character.is_control())
    {
        return false;
    }

    let mut saw_segment = false;
    for segment in file.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return false;
        }
        saw_segment = true;
    }
    saw_segment
}

fn is_absolute_vue_candidate(file: &str) -> bool {
    let bytes = file.as_bytes();
    file.starts_with('/')
        || file.starts_with('\\')
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
        || Path::new(file).is_absolute()
}

fn project_relative_display(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            _ => return None,
        }
    }
    let value = normalized.to_str()?.replace('\\', "/");
    if value.is_empty()
        || value.len() > MAX_RETAINED_VUE_PATH_BYTES
        || !value.ends_with(".vue")
        || value.chars().any(char::is_control)
    {
        return None;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn root_payload(file: String) -> Value {
        serde_json::json!({
            "semantic_tree": {
                "ref": "@e1",
                "tag": "button",
                "sourceHint": {
                    "origin": "vue-dev-instance",
                    "file": file,
                    "component": "VueCard",
                    "signal": "element_parent_component"
                },
                "children": []
            }
        })
    }

    #[test]
    fn recognizes_cross_platform_absolute_vue_candidates() {
        assert!(is_absolute_vue_candidate("/project/src/App.vue"));
        assert!(is_absolute_vue_candidate(r"\project\src\App.vue"));
        assert!(is_absolute_vue_candidate("C:/project/src/App.vue"));
        assert!(is_absolute_vue_candidate(r"C:\project\src\App.vue"));
        assert!(!is_absolute_vue_candidate("src/App.vue"));
    }

    #[tokio::test]
    async fn canonicalizes_in_project_absolute_vue_path_before_retention() {
        let root = std::env::temp_dir().join(format!("localview-vue-authority-{}", Uuid::new_v4()));
        let source_dir = root.join("src");
        fs::create_dir_all(&source_dir).await.unwrap();
        let source = source_dir.join("VueCard.vue");
        fs::write(&source, b"<template />").await.unwrap();

        let absolute = fs::canonicalize(&source)
            .await
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut payload = root_payload(absolute.clone());
        sanitize_vue_snapshot_paths(&mut payload, Some(&root)).await;

        assert_eq!(
            payload["semantic_tree"]["sourceHint"]["file"],
            Value::String("src/VueCard.vue".into())
        );
        assert!(!payload.to_string().contains(&absolute));
        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn scrubs_outside_project_absolute_vue_path() {
        let root = std::env::temp_dir().join(format!("localview-vue-root-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("localview-vue-outside-{}.vue", Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        fs::write(&outside, b"<template />").await.unwrap();

        let absolute = fs::canonicalize(&outside)
            .await
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut payload = root_payload(absolute.clone());
        sanitize_vue_snapshot_paths(&mut payload, Some(&root)).await;

        assert!(payload["semantic_tree"]["sourceHint"].is_null());
        assert!(!payload.to_string().contains(&absolute));
        let _ = fs::remove_dir_all(root).await;
        let _ = fs::remove_file(outside).await;
    }

    #[tokio::test]
    async fn scrubs_component_identity_that_disagrees_with_vue_file() {
        let mut payload = root_payload("src/VueCard.vue".into());
        payload["semantic_tree"]["sourceHint"]["component"] = Value::String("OtherCard".into());

        sanitize_vue_snapshot_paths(&mut payload, None).await;

        assert!(payload["semantic_tree"]["sourceHint"].is_null());
    }

    #[tokio::test]
    async fn scrubs_malformed_vue_uri_before_retention() {
        let raw = "file:///private/VueCard.vue".to_string();
        let mut payload = root_payload(raw.clone());

        sanitize_vue_snapshot_paths(&mut payload, None).await;

        assert!(payload["semantic_tree"]["sourceHint"].is_null());
        assert!(!payload.to_string().contains(&raw));
    }

    #[tokio::test]
    async fn scrubs_absolute_vue_path_when_project_root_is_unavailable() {
        let absolute = if cfg!(windows) {
            "C:/private/VueCard.vue".to_string()
        } else {
            "/private/VueCard.vue".to_string()
        };
        let mut payload = root_payload(absolute.clone());
        sanitize_vue_snapshot_paths(&mut payload, None).await;

        assert!(payload["semantic_tree"]["sourceHint"].is_null());
        assert!(!payload.to_string().contains(&absolute));
    }

    #[tokio::test]
    async fn excess_unique_absolute_paths_are_scrubbed_without_dropping_tree() {
        let mut children = Vec::new();
        for index in 0..(MAX_UNIQUE_ABSOLUTE_VUE_PATHS + 1) {
            let file = if cfg!(windows) {
                format!("C:/private/VueCard{index}.vue")
            } else {
                format!("/private/VueCard{index}.vue")
            };
            children.push(serde_json::json!({
                "sourceHint": {
                    "origin": "vue-dev-instance",
                    "file": file,
                    "component": format!("VueCard{index}"),
                    "signal": "element_parent_component"
                },
                "children": []
            }));
        }
        let mut payload = serde_json::json!({
            "semantic_tree": {
                "sourceHint": null,
                "children": children
            }
        });

        sanitize_vue_snapshot_paths(&mut payload, None).await;

        let tree = payload["semantic_tree"]
            .as_object()
            .expect("tree should remain available");
        let children = tree["children"].as_array().expect("children");
        assert_eq!(children.len(), MAX_UNIQUE_ABSOLUTE_VUE_PATHS + 1);
        assert!(children.iter().all(|child| child["sourceHint"].is_null()));
    }

    #[tokio::test]
    async fn traversal_budget_overflow_drops_the_entire_semantic_tree() {
        let absolute = if cfg!(windows) {
            "C:/private/VueCard.vue".to_string()
        } else {
            "/private/VueCard.vue".to_string()
        };
        let mut node = serde_json::json!({
            "sourceHint": null,
            "children": []
        });
        for _ in 0..=MAX_TREE_DEPTH {
            node = serde_json::json!({
                "sourceHint": null,
                "children": [node]
            });
        }
        let deepest = node
            .pointer_mut(&format!(
                "{}{}",
                "/children/0".repeat(MAX_TREE_DEPTH + 1),
                "/sourceHint"
            ))
            .expect("deep sourceHint");
        *deepest = serde_json::json!({
            "origin": "vue-dev-instance",
            "file": absolute.clone(),
            "component": "VueCard",
            "signal": "element_parent_component"
        });
        let mut payload = serde_json::json!({ "semantic_tree": node });

        sanitize_vue_snapshot_paths(&mut payload, None).await;

        assert!(payload["semantic_tree"].is_null());
        assert!(!payload.to_string().contains(&absolute));
    }

    #[tokio::test]
    async fn leaves_project_relative_vue_ownership_for_projection() {
        let mut payload = root_payload("src/VueCard.vue".into());
        sanitize_vue_snapshot_paths(&mut payload, None).await;

        assert_eq!(
            payload["semantic_tree"]["sourceHint"]["file"],
            Value::String("src/VueCard.vue".into())
        );
    }
}
