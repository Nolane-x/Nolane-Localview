#[test]
fn native_worker_observes_cancellation_before_execution_and_acknowledges_it() {
    let worker = include_str!("../src/native_executor_worker.rs");

    assert!(worker.contains("NativeExecutorCancellationSignal"));
    assert!(worker.contains("/native-executor/cancellations"));
    assert!(worker.contains("acknowledge_cancellation("));

    let loop_start = worker
        .find("for request in requests")
        .expect("native request loop must exist");
    let loop_body = &worker[loop_start..];
    let preflight = loop_body
        .find("cancellation_requested(")
        .expect("worker must check cancellation before native execution");
    let execute = loop_body
        .find("execute_native_visual_packet")
        .expect("native execution must remain explicit");
    assert!(
        preflight < execute,
        "cancellation must be checked before native visual work begins"
    );

    let ack = loop_body
        .find("acknowledge_cancellation(")
        .expect("cancelled work must be acknowledged");
    assert!(preflight < ack);
}

#[test]
fn native_worker_rechecks_cancellation_before_publishing_result() {
    let worker = include_str!("../src/native_executor_worker.rs");
    let loop_start = worker
        .find("for request in requests")
        .expect("native request loop must exist");
    let loop_body = &worker[loop_start..];
    let execute = loop_body
        .find("execute_native_visual_packet")
        .expect("native execution must remain explicit");
    let result_post = loop_body
        .find("post_result(")
        .expect("native result publication must remain explicit");
    let between = &loop_body[execute..result_post];

    assert!(
        between.contains("cancellation_requested("),
        "worker must re-check cancellation after a bounded native call returns and before result publication"
    );
    assert!(between.contains("acknowledge_cancellation("));
}

#[test]
fn cancellation_transport_is_bounded_and_request_scoped() {
    let worker = include_str!("../src/native_executor_worker.rs");

    assert!(worker.contains("MAX_CANCELLATION_SIGNALS"));
    assert!(worker.contains("request.id"));
    assert!(worker.contains("signal.request_id == request_id"));
    assert!(worker.contains("/ack"));
    assert!(!worker.contains("tokio::task::abort"));
    assert!(!worker.contains("abort_handle"));
}
