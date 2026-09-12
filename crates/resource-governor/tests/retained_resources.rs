use localview_resource_governor::{
    RetainedResourceBudget, RetainedResourceKind, RetainedResourceLedger,
};

fn ledger() -> RetainedResourceLedger {
    RetainedResourceLedger::new(RetainedResourceBudget {
        capture_storage_bytes: 100,
        cache_bytes: 40,
    })
    .expect("valid retained budget")
}

#[test]
fn projected_admission_is_dimension_specific_and_exact() {
    let ledger = ledger();
    ledger
        .synchronize(RetainedResourceKind::CaptureStorage, 90)
        .unwrap();
    ledger.synchronize(RetainedResourceKind::Cache, 25).unwrap();
    ledger
        .admit_projected(RetainedResourceKind::CaptureStorage, 100)
        .unwrap();
    let denial = ledger
        .admit_projected(RetainedResourceKind::CaptureStorage, 101)
        .unwrap_err();
    assert_eq!(denial.kind, RetainedResourceKind::CaptureStorage);
    assert_eq!(denial.current_bytes, 90);
    assert_eq!(denial.projected_or_observed_bytes, 101);
    assert_eq!(denial.limit_bytes, 100);
    assert_eq!(ledger.usage().cache_bytes, 25);
}

#[test]
fn synchronize_records_over_limit_reality_before_returning_violation() {
    let ledger = ledger();
    let violation = ledger
        .synchronize(RetainedResourceKind::Cache, 41)
        .unwrap_err();
    assert_eq!(violation.projected_or_observed_bytes, 41);
    assert_eq!(ledger.usage().cache_bytes, 41);
    assert!(
        ledger
            .admit_projected(RetainedResourceKind::Cache, 40)
            .is_err()
    );
}

#[test]
fn failed_projected_admission_does_not_mutate_usage() {
    let ledger = ledger();
    ledger
        .synchronize(RetainedResourceKind::CaptureStorage, 7)
        .unwrap();
    assert!(
        ledger
            .admit_projected(RetainedResourceKind::CaptureStorage, 101)
            .is_err()
    );
    assert_eq!(ledger.usage().capture_storage_bytes, 7);
}

#[test]
fn clones_share_authority_state() {
    let ledger = ledger();
    let clone = ledger.clone();
    ledger
        .synchronize(RetainedResourceKind::CaptureStorage, 33)
        .unwrap();
    assert_eq!(clone.usage().capture_storage_bytes, 33);
}

#[test]
fn zero_limit_budget_is_rejected() {
    assert!(
        RetainedResourceLedger::new(RetainedResourceBudget {
            capture_storage_bytes: 0,
            cache_bytes: 1,
        })
        .is_err()
    );
    assert!(
        RetainedResourceLedger::new(RetainedResourceBudget {
            capture_storage_bytes: 1,
            cache_bytes: 0,
        })
        .is_err()
    );
}
