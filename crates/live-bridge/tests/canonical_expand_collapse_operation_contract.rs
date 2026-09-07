use localview_live_bridge::CanonicalActionOperation;

#[test]
fn canonical_operation_schema_admits_distinct_expand_and_collapse_operations() {
    let expand = serde_json::from_str::<CanonicalActionOperation>("\"expand\"");
    let collapse = serde_json::from_str::<CanonicalActionOperation>("\"collapse\"");

    assert!(
        expand.is_ok(),
        "canonical V4 operation schema must admit server-owned expand authority"
    );
    assert!(
        collapse.is_ok(),
        "canonical V4 operation schema must admit server-owned collapse authority"
    );
    assert_ne!(
        serde_json::to_string(&expand.expect("expand must decode")).unwrap(),
        serde_json::to_string(&collapse.expect("collapse must decode")).unwrap(),
        "Expand and Collapse must remain distinct durable operation identities"
    );
}
