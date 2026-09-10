const WINDOWS_CONSEQUENTIAL_SOURCE: &str = include_str!("../src/windows_consequential.rs");

#[test]
fn missing_operation_binding_consumes_set_value_process_local_authority() {
    let match_start = WINDOWS_CONSEQUENTIAL_SOURCE
        .find("let admitted_operation = match control.journal.admitted_operation(action_id).await")
        .expect("confirmation path must read the admitted operation");
    let match_tail = &WINDOWS_CONSEQUENTIAL_SOURCE[match_start..];
    let missing_start = match_tail
        .find("Ok(None) => {")
        .expect("confirmation path must fail closed when operation binding is missing");
    let missing_tail = &match_tail[missing_start..];
    let missing_end = missing_tail
        .find("Err(error) => {")
        .expect("missing-operation branch must end before journal-error branch");
    let missing_branch = &missing_tail[..missing_end];

    assert!(
        missing_branch.contains("control.set_value")
            && missing_branch.contains("consume_verified"),
        "once generic confirmation is consumed, the missing-operation path must also consume any exact SetValue process-local payload authority so plaintext cannot remain staged and unreachable"
    );
}
