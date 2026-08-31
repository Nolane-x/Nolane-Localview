use std::fs;

#[test]
fn native_worker_uses_exact_request_scoped_cancellation_lookup() {
    let worker = fs::read_to_string("src/native_executor_worker.rs").expect("worker source");

    assert!(
        worker.contains("/native-executor/cancellations/{request_id}"),
        "worker must query cancellation state by exact request id"
    );
    assert!(
        !worker.contains("json::<Vec<NativeExecutorCancellationSignal>>()"),
        "worker must not infer exact cancellation from a bounded batch listing"
    );
}
