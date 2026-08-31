#[test]
fn preview_worker_uses_exact_action_cancellation_transport() {
    let source = include_str!("../src/lib.rs");

    assert!(source.contains("ActionCancellationSignal"));
    assert!(source.contains("async fn preview_action_cancellation("));
    assert!(source.contains("/actions/cancellations/{action_id}"));
    assert!(source.contains("json::<ActionCancellationSignal>()"));
    assert!(source.contains("signal.action_id != action_id"));
    assert!(source.contains("async fn preview_ack_action_cancellation("));
    assert!(source.contains("/actions/cancellations/{action_id}/ack"));

    assert!(source.contains("preview_action_cancellation,"));
    assert!(source.contains("preview_ack_action_cancellation,"));
}

#[test]
fn public_actions_check_cancellation_before_execution_and_before_result_publication() {
    let source = include_str!("../src/lib.rs");
    let loop_start = source
        .find("for (const action of Array.isArray(actions) ? actions : [])")
        .expect("preview action loop must exist");
    let loop_body = &source[loop_start..];

    let execute = loop_body
        .find("await execute(action)")
        .expect("public execution must remain explicit");
    let complete = loop_body
        .find("await complete(invoke, action")
        .expect("result publication must remain explicit");
    assert!(execute < complete);

    let before_execute = &loop_body[..execute];
    assert!(
        before_execute.contains("await cancellationRequested(invoke, action)"),
        "worker must check exact cancellation before executing public action"
    );
    assert!(before_execute.contains("await acknowledgeCancellation(invoke, action)"));

    let between_execute_and_complete = &loop_body[execute..complete];
    assert!(
        between_execute_and_complete.contains("await cancellationRequested(invoke, action)"),
        "worker must re-check cancellation after execution and before publishing a result"
    );
    assert!(between_execute_and_complete.contains("await acknowledgeCancellation(invoke, action)"));
}

#[test]
fn internal_capture_actions_are_excluded_from_public_cancellation_checks() {
    let source = include_str!("../src/lib.rs");

    assert!(source.contains("const isInternalCaptureAction = (queued) =>"));
    assert!(source.contains("action.type === 'freeze_visuals'"));
    assert!(source.contains("action.type === 'restore_visuals'"));
    assert!(source.contains("if (isInternalCaptureAction(action)) return false;"));

    let execute_start = source
        .find("const execute = async (queued) =>")
        .expect("execute helper must exist");
    let execute_end = source[execute_start..]
        .find("const complete = async")
        .map(|offset| execute_start + offset)
        .expect("complete helper must follow execute helper");
    let execute_helper = &source[execute_start..execute_end];
    assert!(execute_helper.contains("case 'freeze_visuals':"));
    assert!(execute_helper.contains("case 'restore_visuals':"));
    assert!(!execute_helper.contains("preview_action_cancellation"));
    assert!(!execute_helper.contains("preview_ack_action_cancellation"));
}
