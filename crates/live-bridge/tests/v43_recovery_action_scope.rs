use localview_live_bridge::{
    ConsequentialRecoveryActionScope, ConsequentialRecoveryInventoryEntry,
    ConsequentialRecoveryState,
};
use uuid::Uuid;

#[test]
fn recovery_scope_freezes_action_identity_independent_of_later_sequence_progress() {
    let first = Uuid::from_u128(0x8701);
    let second = Uuid::from_u128(0x8702);
    let post_boot = Uuid::from_u128(0x8703);
    let inventory = vec![
        ConsequentialRecoveryInventoryEntry {
            action_id: first,
            recovery_state: ConsequentialRecoveryState::DispatchPrepared,
            latest_journal_sequence: 4,
        },
        ConsequentialRecoveryInventoryEntry {
            action_id: second,
            recovery_state: ConsequentialRecoveryState::OutcomeObservedUnverified,
            latest_journal_sequence: 9,
        },
    ];

    let scope = ConsequentialRecoveryActionScope::from_inventory(&inventory);

    assert_eq!(scope.len(), 2);
    assert!(scope.contains(first));
    assert!(scope.contains(second));
    assert!(!scope.contains(post_boot));
    assert!(ConsequentialRecoveryActionScope::from_inventory(&[]).is_empty());
}
