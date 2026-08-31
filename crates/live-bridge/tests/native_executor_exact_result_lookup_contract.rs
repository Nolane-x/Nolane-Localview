use std::{fs, path::PathBuf};

fn source(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(root.join(path)).expect("source file")
}

fn without_whitespace(value: &str) -> String {
    value.chars().filter(|character| !character.is_whitespace()).collect()
}

#[test]
fn exact_native_executor_lookup_is_store_level_not_batch_materialized() {
    let base = source("src/lib.rs");
    assert!(
        base.contains("pub async fn native_executor_result("),
        "base LiveBridge must expose an exact retained-result getter"
    );

    let wrapper = source("src/cancellable_lib.rs");
    let start = wrapper
        .find("pub async fn native_executor_result(")
        .expect("wrapper exact-result method");
    let tail = &wrapper[start..];
    let end = tail
        .find("\n    pub async fn request_native_executor_cancellation(")
        .expect("next wrapper method");
    let method = &tail[..end];
    let compact_method = without_whitespace(method);

    assert!(
        compact_method.contains("self.base.native_executor_result(session_id,request_id).await"),
        "wrapper must delegate exact lookup directly to the base retained store"
    );
    assert!(
        !method.contains("recent_native_executor_results"),
        "exact lookup must not materialize a recent-result batch"
    );
    assert!(
        !method.contains("usize::MAX"),
        "exact lookup must not clone the full bounded result queue"
    );
}
