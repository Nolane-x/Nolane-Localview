from pathlib import Path

path = Path("crates/live-bridge/tests/v43_consequential_journal.rs")
text = path.read_text()
old = '''    let snapshot_cut_ref = format!("cut:postcondition:{receipt_id}");
    assert!(
        bridge
            .record_reconciliation(
                action.session_id,
                ReconciliationSnapshotReceipt {
                    receipt_id: receipt_id.into(),
                    provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
                    target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
                    snapshot_cut_ref: snapshot_cut_ref.clone(),
                    surface_scope: "journal-contract-fixture".into(),
                    completeness: ReconciliationCompleteness::Established,
                    cache_profile_revision: "cache:test:v1".into(),
                    permission_visibility_revision: "visibility:test:v1".into(),
                    capture_sequence: 1,
                    observed_digest: format!("digest:{receipt_id}"),
                    incompleteness_debt: Vec::new(),
                },
            )
            .await
    );
    reconcile_consequential_postconditions(
        &bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt {
            action_id: action.transport_action_id,
            provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
            target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
            snapshot_cut_ref,
            reconciliation_receipt_ref: receipt_id.into(),
            postconditions: vec![ConsequentialPostconditionEvidence {
                contract_ref: "postcondition:message-visible".into(),
                status,
                receipt_ref: format!("postcondition:message-visible:{receipt_id}"),
            }],
        },
    )
    .await
    .unwrap();'''
new = '''    let observation = journal
        .begin_postcondition_observation(action.transport_action_id)
        .await
        .unwrap();
    let snapshot_cut_ref = observation.snapshot_cut_ref().to_owned();
    let snapshot = ReconciliationSnapshotReceipt {
        receipt_id: receipt_id.into(),
        provider_incarnation_ref: action.metadata.provider_incarnation_ref.clone(),
        target_incarnation_ref: action.metadata.target_incarnation_ref.clone(),
        snapshot_cut_ref,
        surface_scope: "journal-contract-fixture".into(),
        completeness: ReconciliationCompleteness::Established,
        cache_profile_revision: "cache:test:v1".into(),
        permission_visibility_revision: "visibility:test:v1".into(),
        capture_sequence: 1,
        observed_digest: format!("digest:{receipt_id}"),
        incompleteness_debt: Vec::new(),
    };
    assert!(bridge
        .record_reconciliation(action.session_id, snapshot.clone())
        .await);
    let observation_receipt = journal
        .complete_postcondition_observation(observation, snapshot)
        .await
        .unwrap();
    reconcile_consequential_postconditions(
        &bridge,
        journal,
        ConsequentialPostconditionReconciliationReceipt::from_observation(
            observation_receipt,
            vec![ConsequentialPostconditionEvidence {
                contract_ref: "postcondition:message-visible".into(),
                status,
                receipt_ref: format!("postcondition:message-visible:{receipt_id}"),
            }],
        ),
    )
    .await
    .unwrap();'''
if text.count(old) != 1:
    raise SystemExit(f"expected one legacy reconciliation helper, found {text.count(old)}")
path.write_text(text.replace(old, new, 1))
