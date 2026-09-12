#[cfg(windows)]
#[path = "support/v43_baseline_cases.rs"]
mod baseline_cases;

#[cfg(windows)]
#[path = "support/v43_follow_on_cases.rs"]
mod follow_on_cases;

#[cfg(windows)]
#[path = "support/v43_verified_input_seed.rs"]
mod verified_input_seed;

#[cfg(windows)]
#[path = "support/v43_verified_input_cases.rs"]
mod verified_input_cases;

#[cfg(windows)]
mod windows_l7_real_provider_campaign {
    use std::{
        collections::BTreeSet,
        fs,
        path::{Path, PathBuf},
    };

    use localview_validation_lab::{
        ActualExecutionAuthority, CampaignLayer, LabMetricKind, LabPreregistration,
        LabRevisionContext, LabRunAdmission, LabRunBuilder, LabSeedIdentity,
        PersistedPreregistrationReceipt, ProviderCampaignKind, ResearchResultClass,
        ResultEvidence, canonical_digest, derive_real_provider_campaign_evidence,
        validate_persisted_receipt,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    use super::{baseline_cases, follow_on_cases, verified_input_cases};

    const PLATFORM_PROFILE: &str = "windows-uia-hosted-r1";
    const COMPARISON_PROFILE: &str = "real-provider-exact-r1";
    const RANDOM_SOURCE_PROFILE: &str = "deterministic-hosted-seed-protocol";
    const CAMPAIGN_START_SEQUENCE: u64 = 100;

    const REQUIRED_CASES: [&str; 9] = [
        "W01-missing-uia-property-event",
        "W02-recreated-uia-element",
        "W03-virtualized-item-realization",
        "W04-unsupported-invoke-pattern",
        "W05-windows-uia-provider-hang",
        "W06-windows-uia-provider-reacquire",
        "W07-foreground-stolen-before-input",
        "W08-partial-input-dispatch",
        "W09-user-held-modifier-interference",
    ];

    fn required_env(name: &'static str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} must be bound by the L7 CI gate"))
    }

    fn environment_value(name: &'static str) -> String {
        std::env::var(name).unwrap_or_else(|_| format!("unknown:{name}-not-exposed"))
    }

    fn executable_digest(env_name: &'static str) -> String {
        let path = required_env(env_name);
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("read exact executable from {env_name}={path}: {error}"));
        let digest = Sha256::digest(bytes);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!("sha256:{hex}")
    }

    fn artifact_dir() -> PathBuf {
        let path = PathBuf::from(required_env("LOCALVIEW_L7_ARTIFACT_DIR"));
        fs::create_dir_all(&path).expect("create bounded L7 artifact directory");
        path
    }

    fn write_value(path: &Path, value: &Value) {
        let bytes = serde_json::to_vec_pretty(value).expect("serialize L7 JSON artifact");
        fs::write(path, bytes).expect("persist L7 JSON artifact");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires hosted Windows UIA providers, both seed executables, and CI environment authority"]
    async fn prospective_l7_campaign_binds_w01_through_w09_to_exact_candidate() {
        assert!(
            std::env::var_os("LOCALVIEW_UIA_SMOKE").is_some(),
            "real-provider campaign execution must be explicitly enabled"
        );

        let candidate_sha = required_env("LOCALVIEW_CANDIDATE_SHA");
        let classic_seed_digest = executable_digest("LOCALVIEW_UIA_SEED_BIN");
        let edge_seed_digest = executable_digest("LOCALVIEW_UIA_EDGE_SEED_BIN");
        let artifact_dir = artifact_dir();
        let architecture = std::env::var("RUNNER_ARCH")
            .unwrap_or_else(|_| std::env::consts::ARCH.to_owned());
        let environment = json!({
            "windows_build": environment_value("LOCALVIEW_WINDOWS_BUILD"),
            "architecture": architecture,
            "localview_candidate_sha": candidate_sha,
            "provider_profile_revision": PLATFORM_PROFILE,
            "display_topology": environment_value("LOCALVIEW_DISPLAY_TOPOLOGY"),
            "dpi_scale": environment_value("LOCALVIEW_DPI_SCALE"),
            "locale_input_method": environment_value("LOCALVIEW_LOCALE_INPUT_METHOD"),
            "permission_state": environment_value("LOCALVIEW_PERMISSION_STATE"),
            "classic_seed_executable_digest": classic_seed_digest,
            "edge_seed_executable_digest": edge_seed_digest,
            "w08_partial_evidence_source": "deterministic-wrapper-through-production-boundary",
            "w08_natural_windows_partial_observed": false,
            "w08_real_platform_companion_evidence": "production-SendInput-full-dispatch-through-same-receipt-path",
            "required_cases": REQUIRED_CASES,
        });
        write_value(&artifact_dir.join("environment-manifest.json"), &environment);
        let environment_digest =
            canonical_digest(&environment).expect("digest canonical nine-seed environment manifest");

        let seed_catalog_digest = canonical_digest(&json!({
            "candidate_sha": required_env("LOCALVIEW_CANDIDATE_SHA"),
            "environment_digest": environment_digest.0.clone(),
            "classic_seed_executable_digest": classic_seed_digest.clone(),
            "edge_seed_executable_digest": edge_seed_digest.clone(),
            "w08_partial_evidence_source": "deterministic-wrapper-through-production-boundary",
            "w08_natural_windows_partial_observed": false,
            "required_cases": REQUIRED_CASES,
        }))
        .expect("digest prospective W01-W09 L7 seed catalog");

        let preregistration = LabPreregistration {
            revision_context: LabRevisionContext {
                lab_revision: "lab-v43-windows-l7-r3".into(),
                seed_corpus_revision: "windows-provider-seeds-w01-w09-r3".into(),
                spec_revision_digest: "v4.3-principal-provider-reconciliation-closure".into(),
                reference_reducer_revision: "provider-oracle-r1".into(),
                mutation_catalog_revision: "windows-provider-seed-matrix-r1".into(),
                comparison_profile_revision: COMPARISON_PROFILE.into(),
                random_source_profile: RANDOM_SOURCE_PROFILE.into(),
                platform_profile: Some(PLATFORM_PROFILE.into()),
                start_sequence: CAMPAIGN_START_SEQUENCE,
            },
            seed_catalog_digest: seed_catalog_digest.clone(),
            seed_identities: vec![
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[0].into(),
                    prediction_revision: "w01-missing-property-event-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[1].into(),
                    prediction_revision: "w02-recreated-element-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[2].into(),
                    prediction_revision: "w03-virtualized-realization-r1".into(),
                    oracle_revision: "independent-wpf-oracle-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[3].into(),
                    prediction_revision: "w04-unsupported-invoke-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[4].into(),
                    prediction_revision: "w05-provider-hang-r1".into(),
                    oracle_revision: "independent-wpf-oracle-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[5].into(),
                    prediction_revision: "w06-provider-reacquire-r1".into(),
                    oracle_revision: "independent-seed-pipe-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[6].into(),
                    prediction_revision: "w07-foreground-stolen-r1".into(),
                    oracle_revision: "independent-wpf-input-oracle-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[7].into(),
                    prediction_revision: "w08-partial-input-dispatch-r1".into(),
                    oracle_revision:
                        "deterministic-partial-wrapper-plus-independent-wpf-full-smoke-r1".into(),
                },
                LabSeedIdentity {
                    seed_id: REQUIRED_CASES[8].into(),
                    prediction_revision: "w09-modifier-interference-r1".into(),
                    oracle_revision: "independent-wpf-input-oracle-r1".into(),
                },
            ],
            campaign_layer: CampaignLayer::L7,
            expected_distinctions: BTreeSet::from([
                "event continuity != current snapshot completeness".into(),
                "element incarnation != provider incarnation".into(),
                "provider realization receipt != fresh action authority".into(),
                "unsupported semantic pattern != successful dispatch".into(),
                "provider timeout != reusable worker authority".into(),
                "provider reacquire invalidates old authority".into(),
                "preflight foreground != final input-boundary foreground".into(),
                "partial platform insertion != retry authority".into(),
                "user-held modifier != LocalView normalization authority".into(),
            ]),
            model_bound: None,
            assumptions: BTreeSet::from([
                "hosted Windows runner exposes real Win32 UI Automation".into(),
                "hosted Windows runner supports deterministic WPF UI Automation".into(),
                "production LocalView never reads either seed oracle channel".into(),
                "W08 partial insertion count is deterministic wrapper evidence through the production boundary and is not claimed as a naturally observed hosted-Windows partial SendInput result".into(),
                "W08 separately executes the real production SendInput backend for ordinary full insertion through the same verified receipt path".into(),
            ]),
            declared_metrics: BTreeSet::from([
                LabMetricKind::Rpomr,
                LabMetricKind::Eoffr,
                LabMetricKind::Rmr,
                LabMetricKind::Piaer,
                LabMetricKind::Scar,
                LabMetricKind::Cbfr,
            ]),
            creation_sequence: 80,
        };

        let prepared = preregistration
            .prepare()
            .expect("prepare nine-seed L7 preregistration");
        let prereg_path = artifact_dir.join("LAB-PREREGISTRATION.json");
        fs::write(&prereg_path, &prepared.canonical_bytes)
            .expect("persist preregistration before campaign start");
        let persisted_receipt = PersistedPreregistrationReceipt {
            digest: prepared.digest.clone(),
            logical_sequence: 90,
            persistence_ref: prereg_path.to_string_lossy().into_owned(),
        };
        fs::write(
            artifact_dir.join("LAB-PREREGISTRATION-RECEIPT.json"),
            serde_json::to_vec_pretty(&persisted_receipt)
                .expect("serialize preregistration receipt"),
        )
        .expect("persist preregistration receipt");
        let receipt = validate_persisted_receipt(&prepared, persisted_receipt)
            .expect("validate persisted preregistration receipt");
        let authority = ActualExecutionAuthority {
            seed_catalog_digest,
            comparison_profile_revision: COMPARISON_PROFILE.into(),
            random_source_profile: RANDOM_SOURCE_PROFILE.into(),
            model_bound: None,
        };
        let admission = LabRunAdmission::Prospective {
            preregistration: preregistration.clone(),
            receipt,
        };
        let mut run = LabRunBuilder::start_provider_campaign(
            ProviderCampaignKind::RealProviderSeedApplications,
            admission,
            authority,
        )
        .expect("start typed prospective W01-W09 provider campaign");

        let environment_digest_text = environment_digest.0.clone();
        let records = vec![
            baseline_cases::run_w01(
                &classic_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                101,
            )
            .await,
            baseline_cases::run_w02(
                &classic_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                102,
            )
            .await,
            follow_on_cases::run_w03(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                103,
            )
            .await,
            follow_on_cases::run_w04(
                &classic_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                104,
            ),
            follow_on_cases::run_w05(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                105,
            ),
            baseline_cases::run_w06(
                &classic_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                106,
            )
            .await,
            verified_input_cases::run_w07(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                107,
            )
            .await,
            verified_input_cases::run_w08(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                108,
            )
            .await,
            verified_input_cases::run_w09(
                &edge_seed_digest,
                &environment_digest_text,
                PLATFORM_PROFILE,
                COMPARISON_PROFILE,
                109,
            )
            .await,
        ];

        let observed_seed_ids = records
            .iter()
            .map(|record| {
                record
                    .observation
                    .seed_id
                    .clone()
                    .expect("every L7 provider observation must bind a seed identity")
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            observed_seed_ids,
            REQUIRED_CASES
                .iter()
                .map(|seed| (*seed).to_owned())
                .collect::<BTreeSet<_>>(),
            "the prospective campaign must execute exactly W01 through W09"
        );

        for record in &records {
            assert_eq!(
                record.result_evidence,
                Some(ResultEvidence::RealProviderIntegrationPass),
                "every required W01-W09 case must be complete before campaign finalization"
            );
            assert!(record.observation.provider_backed);
            assert!(record.observation.failure_flags.is_empty());
            run.append_observation(record.observation.clone())
                .expect("append exact provider-backed observation to prospective campaign");
        }

        let campaign_evidence = derive_real_provider_campaign_evidence(&records)
            .expect("all nine provider records must produce campaign evidence");
        assert_eq!(
            campaign_evidence,
            ResultEvidence::RealProviderIntegrationPass
        );
        let completed = run
            .finalize(campaign_evidence, 110)
            .expect("clean measured W01-W09 L7 campaign may mint scoped provider pass");
        let rpomr = completed
            .payload
            .metric_snapshot
            .get(LabMetricKind::Rpomr)
            .expect("RPOMR metric must exist");
        assert_eq!((rpomr.numerator, rpomr.denominator), (0, 9));
        assert_eq!(
            completed.payload.result_class,
            ResearchResultClass::RealProviderIntegrationPass
        );
        assert_eq!(completed.payload.observation_digests.len(), 9);
        assert_eq!(
            completed
                .payload
                .actual_execution_authority
                .seed_catalog_digest,
            preregistration.seed_catalog_digest
        );
        assert_eq!(
            completed.payload.revision_context.platform_profile.as_deref(),
            Some(PLATFORM_PROFILE)
        );
        fs::write(
            artifact_dir.join("LAB-RESULT.json"),
            serde_json::to_vec_pretty(&completed).expect("serialize completed W01-W09 result"),
        )
        .expect("persist completed nine-seed L7 result artifact");
    }
}
