#[cfg(windows)]
mod windows_l7_real_provider_campaign_w01_w06 {
    use std::collections::BTreeSet;

    use localview_validation_lab::{
        ActualExecutionAuthority, CampaignLayer, CanonicalDigest, LabMetricKind, LabObservation,
        LabPreregistration, LabRevisionContext, LabRunAdmission, LabRunBuilder, LabSeedIdentity,
        PersistedPreregistrationReceipt, ProviderCampaignKind, ResearchResultClass, ResultEvidence,
        validate_persisted_receipt,
    };

    const PLATFORM_PROFILE: &str = "windows-uia-hosted-r1";
    const COMPARISON_PROFILE: &str = "real-provider-exact-r1";
    const RANDOM_SOURCE_PROFILE: &str = "deterministic-hosted-seed-protocol";

    fn required_seed_identities() -> Vec<LabSeedIdentity> {
        [
            (
                "W01-missing-uia-property-event",
                "w01-missing-property-event-r1",
                "independent-seed-pipe-r1",
            ),
            (
                "W02-recreated-uia-element",
                "w02-recreated-element-r1",
                "independent-seed-pipe-r1",
            ),
            (
                "W03-virtualized-item-realization",
                "w03-virtualized-item-r1",
                "independent-wpf-oracle-r1",
            ),
            (
                "W04-unsupported-invoke-pattern",
                "w04-unsupported-invoke-r1",
                "independent-seed-pipe-r1",
            ),
            (
                "W05-windows-uia-provider-hang",
                "w05-provider-hang-r1",
                "independent-wpf-oracle-r1",
            ),
            (
                "W06-windows-uia-provider-reacquire",
                "w06-provider-reacquire-r1",
                "independent-seed-pipe-r1",
            ),
        ]
        .into_iter()
        .map(|(seed_id, prediction_revision, oracle_revision)| LabSeedIdentity {
            seed_id: seed_id.into(),
            prediction_revision: prediction_revision.into(),
            oracle_revision: oracle_revision.into(),
        })
        .collect()
    }

    fn observation(seed_id: &str, logical_sequence: u64) -> LabObservation {
        LabObservation {
            observation_id: format!("real_provider:{seed_id}"),
            seed_id: Some(seed_id.into()),
            expected_outcome: "bounded-correct".into(),
            observed_outcome: "bounded-correct".into(),
            principal_expected: None,
            principal_dispatched: None,
            eligible_metrics: BTreeSet::from([LabMetricKind::Rpomr]),
            failure_flags: BTreeSet::new(),
            evidence_refs: BTreeSet::from([format!("campaign-coverage:{seed_id}")]),
            provider_backed: true,
            comparison_profile_revision: COMPARISON_PROFILE.into(),
            logical_sequence,
        }
    }

    #[test]
    fn prospective_l7_campaign_requires_all_w01_through_w06_observations() {
        let seed_catalog_digest = CanonicalDigest("sha256:task9-six-seed-coverage-r1".into());
        let preregistration = LabPreregistration {
            revision_context: LabRevisionContext {
                lab_revision: "lab-v43-windows-l7-six-seed-r2".into(),
                seed_corpus_revision: "windows-provider-seeds-w01-w06-r2".into(),
                spec_revision_digest: "v4.3-principal-provider-reconciliation-closure".into(),
                reference_reducer_revision: "provider-oracle-r1".into(),
                mutation_catalog_revision: "windows-provider-seed-matrix-r1".into(),
                comparison_profile_revision: COMPARISON_PROFILE.into(),
                random_source_profile: RANDOM_SOURCE_PROFILE.into(),
                platform_profile: Some(PLATFORM_PROFILE.into()),
                start_sequence: 100,
            },
            seed_catalog_digest: seed_catalog_digest.clone(),
            seed_identities: required_seed_identities(),
            campaign_layer: CampaignLayer::L7,
            expected_distinctions: BTreeSet::from([
                "virtual placeholder != realized current authority".into(),
                "unsupported pattern != dispatch authority".into(),
                "timed-out provider worker != reusable provider authority".into(),
                "provider reacquire invalidates old authority".into(),
            ]),
            model_bound: None,
            assumptions: BTreeSet::from([
                "real Windows UI Automation provider path is required".into(),
                "production LocalView never reads either seed oracle channel".into(),
            ]),
            declared_metrics: BTreeSet::from([LabMetricKind::Rpomr]),
            creation_sequence: 80,
        };
        let prepared = preregistration
            .prepare()
            .expect("prepare six-seed preregistration");
        let receipt = validate_persisted_receipt(
            &prepared,
            PersistedPreregistrationReceipt {
                digest: prepared.digest.clone(),
                logical_sequence: 90,
                persistence_ref: "artifact:task9-six-seed-coverage-r1".into(),
            },
        )
        .expect("validate six-seed preregistration receipt");
        let authority = ActualExecutionAuthority {
            seed_catalog_digest,
            comparison_profile_revision: COMPARISON_PROFILE.into(),
            random_source_profile: RANDOM_SOURCE_PROFILE.into(),
            model_bound: None,
        };
        let mut run = LabRunBuilder::start_provider_campaign(
            ProviderCampaignKind::RealProviderSeedApplications,
            LabRunAdmission::Prospective {
                preregistration,
                receipt,
            },
            authority,
        )
        .expect("start six-seed prospective L7 campaign");

        // Preserve the established W01/W02/W06 logical sequences and append the
        // newly closed W03/W04/W05 observations afterwards. The dedicated
        // workflow executes every standalone real-provider gate before this
        // coverage contract, so this test verifies the Lab admission/finalize
        // boundary rather than reimplementing provider behavior with fakes.
        for (seed_id, sequence) in [
            ("W01-missing-uia-property-event", 101),
            ("W02-recreated-uia-element", 102),
            ("W06-windows-uia-provider-reacquire", 103),
            ("W03-virtualized-item-realization", 104),
            ("W04-unsupported-invoke-pattern", 105),
            ("W05-windows-uia-provider-hang", 106),
        ] {
            run.append_observation(observation(seed_id, sequence))
                .expect("append required six-seed provider-backed observation");
        }

        let completed = run
            .finalize(ResultEvidence::RealProviderIntegrationPass, 107)
            .expect("all six required observations may finalize the bounded campaign");
        let rpomr = completed
            .payload
            .metric_snapshot
            .get(LabMetricKind::Rpomr)
            .expect("six-seed campaign must report RPOMR");
        assert_eq!((rpomr.numerator, rpomr.denominator), (0, 6));
        assert_eq!(
            completed.payload.result_class,
            ResearchResultClass::RealProviderIntegrationPass
        );
        assert_eq!(completed.payload.observations.len(), 6);
    }
}
