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
        .or_else(|| source.find("const processPendingAction = async"))
        .expect("preview action processing loop must exist");
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

    let between_execute_and_complete = &loop_body[execute..complete];
    assert!(
        between_execute_and_complete.contains("await cancellationRequested(invoke, action)"),
        "worker must re-check cancellation after execution and before publishing a result"
    );
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

#[test]
fn taken_actions_remain_worker_owned_until_ack_or_result_is_terminal() {
    let source = include_str!("../src/lib.rs");

    assert!(
        source.contains("const pendingActions = new Map();"),
        "taken actions need worker-owned retry state"
    );
    assert!(source.contains("const rememberTakenActions = (actions) =>"));
    assert!(source.contains("pendingActions.set(action.id, {"));
    assert!(source.contains("for (const entry of pendingActions.values())"));
    assert!(
        source.contains("if (pendingActions.size === 0)"),
        "do not take a new batch while an earlier batch is transport-unsettled"
    );
    assert!(source.contains("pendingActions.delete(action.id);"));
}

#[test]
fn post_execution_transport_retries_never_execute_the_action_twice() {
    let source = include_str!("../src/lib.rs");
    let process_start = source
        .find("const processPendingAction = async")
        .expect("pending action state machine must exist");
    let process_end = source[process_start..]
        .find("const tick = async")
        .map(|offset| process_start + offset)
        .expect("pending action processor must precede tick");
    let process = &source[process_start..process_end];

    assert!(process.contains("if (!entry.executed) {"));
    assert!(process.contains("entry.payload = await execute(action);"));
    assert!(process.contains("finally {"));
    assert!(process.contains("entry.executed = true;"));
    assert!(process.contains("await complete(invoke, action, entry.ok, entry.payload, entry.actionError);"));
}

#[test]
fn observed_cancellation_retries_ack_without_falling_back_to_result_publication() {
    let source = include_str!("../src/lib.rs");
    let process_start = source
        .find("const processPendingAction = async")
        .expect("pending action state machine must exist");
    let process_end = source[process_start..]
        .find("const tick = async")
        .map(|offset| process_start + offset)
        .expect("pending action processor must precede tick");
    let process = &source[process_start..process_end];

    assert!(process.contains("cancellationSeen: false"));
    assert!(process.contains("entry.cancellationSeen = true;"));
    assert!(process.contains("if (entry.cancellationSeen) {"));
    assert!(process.contains("await acknowledgeCancellation(invoke, action);"));
    assert!(process.contains("pendingActions.delete(action.id);"));
}

#[test]
fn already_terminal_result_conflict_does_not_create_an_endless_retry() {
    let source = include_str!("../src/lib.rs");
    let command_start = source
        .find("async fn preview_complete_action(")
        .expect("result command must exist");
    let command_end = source[command_start..]
        .find("fn control_client()")
        .map(|offset| command_start + offset)
        .expect("control client helper must follow result command");
    let command = &source[command_start..command_end];

    assert!(command.contains("reqwest::StatusCode::CONFLICT"));
    assert!(command.contains("return Ok(())"));
}
