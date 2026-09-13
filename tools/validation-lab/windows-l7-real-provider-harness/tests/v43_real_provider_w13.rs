#[cfg(windows)]
#[path = "support/v43_sensitive_field_seed.rs"]
mod sensitive_field_seed;

#[cfg(windows)]
mod windows_real_provider_w13 {
    use std::{fs, path::PathBuf};

    use serde_json::json;
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    use super::sensitive_field_seed::{
        SENSITIVE_FIELD_AUTOMATION_ID, SensitiveEdgeSeedProcess, attach_and_snapshot, spawn_worker,
        truth_u64,
    };

    #[test]
    #[ignore = "requires a real interactive Windows UI Automation provider and WPF edge seed"]
    fn w13_sensitive_password_never_enters_semantic_snapshot_or_artifact() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider W13 execution must be explicitly enabled"
        );

        let canary = format!("LOCALVIEW_W13_SECRET_{}", Uuid::new_v4());
        let mut seed = SensitiveEdgeSeedProcess::spawn();
        let prepared = seed.prepare_sensitive_field(&canary);
        assert!(
            !prepared.to_string().contains(&canary),
            "test-only ground-truth side channel must never echo W13 plaintext"
        );
        let target_window = truth_u64(&prepared, "window_handle");
        assert_ne!(target_window, 0, "W13 target HWND must be real");
        assert_eq!(truth_u64(&prepared, "value_read_count"), 0);

        let worker = spawn_worker();
        let (_attachment, snapshot) = attach_and_snapshot(
            &worker,
            &seed,
            target_window,
            "cut:v43:w13:sensitive-field",
        );

        let node = snapshot
            .nodes()
            .iter()
            .find(|node| node.automation_id.as_deref() == Some(SENSITIVE_FIELD_AUTOMATION_ID))
            .expect("shipping Windows UIA snapshot must retain W13 protected field metadata");
        assert_eq!(
            node.attributes.get("windows_uia.is_password").map(String::as_str),
            Some("true"),
            "real UIA provider must mark the seed field as password-protected"
        );
        assert_eq!(
            node.attributes
                .get("windows_uia.semantic_text_protection")
                .map(String::as_str),
            Some("protected_password"),
            "shipping snapshot must carry an explicit protection fact instead of plaintext"
        );

        let serialized_snapshot =
            serde_json::to_string(snapshot.as_ref()).expect("serialize W13 semantic snapshot");
        assert!(
            !serialized_snapshot.contains(&canary),
            "W13 canary must not appear in any serialized node field, locator hint, attribute, or snapshot metadata"
        );

        let after = seed.sensitive_field_state();
        assert!(
            !after.to_string().contains(&canary),
            "ground-truth state response must remain secret-safe"
        );
        let read_count = truth_u64(&after, "value_read_count");
        assert_eq!(
            read_count, 0,
            "shipping semantic observation must not call IValueProvider.Value for a protected password field"
        );

        let canary_digest = format!("{:x}", Sha256::digest(canary.as_bytes()));
        let record = json!({
            "schema": "localview-v43-w13-real-provider-record-v1",
            "candidate_sha": std::env::var("LOCALVIEW_CANDIDATE_SHA").unwrap_or_else(|_| "local-unbound".into()),
            "case_id": "W13",
            "target_automation_id": SENSITIVE_FIELD_AUTOMATION_ID,
            "provider_is_password": true,
            "semantic_text_protection": "protected_password",
            "secret_length": canary.len(),
            "secret_sha256": canary_digest,
            "provider_value_read_count": read_count,
            "semantic_snapshot_contains_plaintext": false,
            "oracle_response_contains_plaintext": false,
        });
        let serialized_record =
            serde_json::to_string_pretty(&record).expect("serialize W13 evidence record");
        assert!(
            !serialized_record.contains(&canary),
            "W13 evidence artifact must bind only digest/protection facts, never plaintext"
        );

        if let Some(dir) = std::env::var_os("LOCALVIEW_L7_ARTIFACT_DIR") {
            let dir = PathBuf::from(dir);
            fs::create_dir_all(&dir).expect("create W13 artifact directory");
            fs::write(dir.join("W13-REAL-PROVIDER-RECORD.json"), serialized_record)
                .expect("write W13 real-provider evidence record");
        }

        seed.shutdown();
    }
}
