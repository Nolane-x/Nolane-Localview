use localview_windows_uia_provider::{
    WindowsUiaPattern, WindowsUiaPatternDispatchOperation,
};

#[test]
fn expand_and_collapse_remain_distinct_provider_operations_for_one_pattern() {
    assert_ne!(
        WindowsUiaPatternDispatchOperation::Expand,
        WindowsUiaPatternDispatchOperation::Collapse,
        "the provider boundary must never collapse Expand and Collapse into one ambiguous operation"
    );
    assert_eq!(
        WindowsUiaPatternDispatchOperation::Expand.required_pattern(),
        WindowsUiaPattern::ExpandCollapse
    );
    assert_eq!(
        WindowsUiaPatternDispatchOperation::Collapse.required_pattern(),
        WindowsUiaPattern::ExpandCollapse
    );
}
