use chrono::Utc;
use localview_live_bridge::{
    LiveBridge, NetworkFaultControlCommand, NetworkFaultControlResult,
};
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn network_fault_control_is_private_session_scoped_and_sanitized() {
    let bridge = LiveBridge::new(32, 8);
    let session_id = Uuid::new_v4();
    let other_session = Uuid::new_v4();
    let lease_token = Uuid::new_v4();

    let request = bridge
        .enqueue_network_fault_control(
            session_id,
            NetworkFaultControlCommand::Install {
                lease_token,
                plan: json!({
                    "fingerprint": "0123456789abcdef",
                    "lease_ms": 1000,
                    "rules": [{
                        "id": "fail-data",
                        "transport": "fetch",
                        "method": "get",
                        "path": "/api/data",
                        "effect": {"kind": "fail"},
                        "max_hits": 1
                    }]
                }),
            },
        )
        .await;

    assert!(
        bridge.take_public_actions(session_id, 8).await.is_empty(),
        "network fault control must never enter the public action queue"
    );

    let controls = bridge.take_network_fault_controls(session_id, 8).await;
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].id, request.id);
    assert!(matches!(
        controls[0].command,
        NetworkFaultControlCommand::Install { .. }
    ));

    assert!(
        bridge
            .claim_network_fault_control(other_session, request.id)
            .await
            .is_none(),
        "another session must not claim private control"
    );
    let claimed = bridge
        .claim_network_fault_control(session_id, request.id)
        .await
        .expect("exact session must claim its private control");

    assert!(
        !bridge
            .complete_network_fault_control(
                other_session,
                NetworkFaultControlResult {
                    request_id: request.id,
                    ok: true,
                    error: None,
                    payload: json!({"active": true}),
                    completed_at: Utc::now(),
                },
            )
            .await,
        "another session must not complete private control"
    );

    assert!(
        bridge
            .complete_network_fault_control(
                session_id,
                NetworkFaultControlResult {
                    request_id: request.id,
                    ok: true,
                    error: None,
                    payload: json!({
                        "active": true,
                        "fingerprint": "0123456789abcdef",
                        "rule_count": 1,
                        "total_hits": 0,
                        "remaining_ms": 900,
                        "lease_token": lease_token,
                        "plan": {"secret": "must-not-escape"},
                        "arbitrary": "must-not-escape"
                    }),
                    completed_at: Utc::now(),
                },
            )
            .await,
        "claimed private control must complete once"
    );

    let stored = bridge
        .network_fault_control_result(session_id, claimed.id)
        .await
        .expect("sanitized result must be retained");
    assert!(stored.ok);
    assert_eq!(stored.payload["active"], true);
    assert_eq!(stored.payload["fingerprint"], "0123456789abcdef");
    assert_eq!(stored.payload["rule_count"], 1);
    assert_eq!(stored.payload["total_hits"], 0);
    assert_eq!(stored.payload["remaining_ms"], 900);

    let encoded = stored.payload.to_string();
    assert!(!encoded.contains("lease_token"));
    assert!(!encoded.contains(&lease_token.to_string()));
    assert!(!encoded.contains("plan"));
    assert!(!encoded.contains("must-not-escape"));
    assert!(!encoded.contains("arbitrary"));

    bridge.release_session(session_id).await;
    assert!(
        bridge
            .network_fault_control_result(session_id, request.id)
            .await
            .is_none(),
        "session cleanup must clear private network-fault authority"
    );
}
