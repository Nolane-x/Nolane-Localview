#![cfg(target_os = "linux")]

mod linux_real_provider_l04 {
    use std::{
        collections::{HashSet, VecDeque},
        fs,
        io::{BufRead, BufReader, Write},
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::Duration,
    };

    use atspi::{
        AccessibilityConnection, Event, ObjectEvents, ObjectRefOwned, State,
        proxy::accessible::AccessibleProxy,
    };
    use localview_linux_atspi_provider::{
        AtspiEndpoint, AtspiEventAssurance, AtspiEventReliabilityProfile,
        AtspiObservationOrigin, AtspiSemanticDimension, LinuxAtspiProvider,
    };
    use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
    use serde_json::{Value, json};
    use tokio::time::{Instant, sleep, timeout};
    use tokio_stream::StreamExt;

    const ACCESSIBLE_NAME: &str = "LocalView L04 Missed Event Button";

    struct SeedProcess {
        child: Child,
        stdout: BufReader<std::process::ChildStdout>,
    }

    impl SeedProcess {
        fn launch() -> Self {
            let seed_bin = std::env::var("LOCALVIEW_L04_SEED_BIN")
                .expect("LOCALVIEW_L04_SEED_BIN must point to the compiled real GTK3 seed");
            let mut child = Command::new(seed_bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch real GTK3 L04 seed");
            let stdout = BufReader::new(child.stdout.take().expect("seed stdout"));
            let mut seed = Self { child, stdout };
            let ready = seed.read_event();
            assert_eq!(ready["event"], "ready", "GTK L04 seed must announce readiness");
            seed
        }

        fn command(&mut self, command: &str) -> Value {
            let stdin = self.child.stdin.as_mut().expect("seed stdin");
            writeln!(stdin, "{command}").expect("write seed command");
            stdin.flush().expect("flush seed command");
            self.read_event()
        }

        fn read_event(&mut self) -> Value {
            let mut line = String::new();
            self.stdout
                .read_line(&mut line)
                .expect("read JSON event from GTK seed");
            assert!(!line.is_empty(), "GTK seed terminated before emitting JSON");
            serde_json::from_str(&line).expect("GTK seed stdout must contain JSON only")
        }
    }

    impl Drop for SeedProcess {
        fn drop(&mut self) {
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    async fn proxy_for<'a>(
        connection: &'a atspi::zbus::Connection,
        object: &ObjectRefOwned,
    ) -> Option<AccessibleProxy<'a>> {
        let bus_name = object.name_as_str()?.to_owned();
        let path = object.path_as_str().to_owned();
        AccessibleProxy::builder(connection)
            .destination(bus_name)
            .ok()?
            .path(path)
            .ok()?
            .build()
            .await
            .ok()
    }

    async fn find_accessible_by_name(
        connection: &AccessibilityConnection,
        expected_name: &str,
    ) -> ObjectRefOwned {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let root = connection
                .root_accessible_on_registry()
                .await
                .expect("AT-SPI registry root must be available");
            let mut queue: VecDeque<ObjectRefOwned> = root
                .get_children()
                .await
                .expect("AT-SPI registry children")
                .into();
            let mut seen = HashSet::new();

            while let Some(object) = queue.pop_front() {
                if object.is_null() {
                    continue;
                }
                let key = (
                    object.name_as_str().unwrap_or_default().to_owned(),
                    object.path_as_str().to_owned(),
                );
                if !seen.insert(key) {
                    continue;
                }

                let Some(proxy) = proxy_for(connection.connection(), &object).await else {
                    continue;
                };
                if proxy.name().await.ok().as_deref() == Some(expected_name) {
                    return object;
                }
                if let Ok(children) = proxy.get_children().await {
                    queue.extend(children);
                }
            }

            assert!(
                Instant::now() < deadline,
                "timed out finding {expected_name:?} through the real AT-SPI registry"
            );
            sleep(Duration::from_millis(100)).await;
        }
    }

    fn same_object(left: &ObjectRefOwned, right: &ObjectRefOwned) -> bool {
        left.name_as_str() == right.name_as_str() && left.path_as_str() == right.path_as_str()
    }

    fn write_evidence(payload: &Value) {
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("LOCALVIEW_L7_ARTIFACT_DIR must be set by the real-provider workflow"),
        );
        fs::create_dir_all(&artifact_dir).expect("create L04 artifact directory");
        fs::write(
            artifact_dir.join("l04-real-provider-evidence.json"),
            serde_json::to_vec_pretty(payload).expect("serialize L04 evidence"),
        )
        .expect("write L04 evidence");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real Ubuntu GTK/ATK/AT-SPI seed"]
    async fn l04_real_missed_event_requires_direct_reconciliation() {
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("LOCALVIEW_CANDIDATE_SHA must bind evidence to the exact candidate");
        let mut seed = SeedProcess::launch();

        let observer = AccessibilityConnection::new()
            .await
            .expect("connect independent real AT-SPI observer");
        observer
            .register_event::<ObjectEvents>()
            .await
            .expect("register real AT-SPI object-event listener");
        let object = find_accessible_by_name(&observer, ACCESSIBLE_NAME).await;
        let bus_name = object
            .name_as_str()
            .expect("real accessible must have a unique bus name")
            .to_owned();
        let object_path = object.path_as_str().to_owned();
        let proxy = proxy_for(observer.connection(), &object)
            .await
            .expect("build real L04 accessible proxy");

        let initial_states = proxy.get_state().await.expect("read initial real AT-SPI state");
        let initial_checked = initial_states.contains(State::Checked);
        assert!(!initial_checked, "L04 seed must begin unchecked");

        let provider = LinuxAtspiProvider::connect(
            ProviderIncarnationRef::from("provider:linux-atspi:real:l04"),
            TargetIncarnationRef::from("target:linux-atspi:real:l04"),
        )
        .await
        .expect("connect shipping Linux AT-SPI provider");
        let binding = provider
            .bind_initial(
                AtspiEndpoint::new(bus_name, object_path),
                "cut:l04:real:initial",
            )
            .expect("initial real L04 binding must be unique");

        let reliability = AtspiEventReliabilityProfile::linux_toolkit_default([
            AtspiSemanticDimension::StateSet,
        ]);
        assert_eq!(reliability.assurance(), AtspiEventAssurance::Incomplete);
        assert!(reliability.requires_direct_reconciliation_for(AtspiSemanticDimension::StateSet));

        let old_observation = provider
            .reconcile_action_state(&binding)
            .await
            .expect("initial direct reconciliation must succeed");
        assert_eq!(
            old_observation.origin(),
            AtspiObservationOrigin::DirectReconciliation
        );
        assert!(!old_observation.states().contains(State::Checked));
        let old_revision = old_observation.observation_revision();
        let old_cut = old_observation.snapshot_cut_ref().to_owned();

        let mut events = observer.event_stream();

        let control = seed.command("emit_control");
        assert_eq!(control["event"], "control_emitted");
        let positive_control_state_event_observed = timeout(Duration::from_secs(2), async {
            while let Some(event) = events.next().await {
                let Ok(Event::Object(ObjectEvents::StateChanged(event))) = event else {
                    continue;
                };
                if same_object(&event.item, &object)
                    && event.state == State::Busy
                    && event.enabled
                {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(
            positive_control_state_event_observed,
            "independent listener must observe a positive-control state event from the exact seed object"
        );

        let mutated = seed.command("mutate_silent");
        assert_eq!(mutated["event"], "mutated_silent");

        let checked_event_observed = timeout(Duration::from_millis(750), async {
            while let Some(event) = events.next().await {
                let Ok(Event::Object(ObjectEvents::StateChanged(event))) = event else {
                    continue;
                };
                if same_object(&event.item, &object) && event.state == State::Checked {
                    return true;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(
            !checked_event_observed,
            "silent mutation must reproduce the missing-toolkit-event condition"
        );

        let independent_states = proxy
            .get_state()
            .await
            .expect("independent direct AT-SPI state query after silent mutation");
        let independent_direct_checked = independent_states.contains(State::Checked);
        assert!(
            independent_direct_checked,
            "real direct AT-SPI state must expose the silently changed CHECKED state"
        );

        let reconciled = provider
            .reconcile_action_state(&binding)
            .await
            .expect("shipping direct reconciliation after missed event");
        let provider_reconciled_checked = reconciled.states().contains(State::Checked);
        let fresh_observation_revision_greater_than_old =
            reconciled.observation_revision() > old_revision;
        let old_observation_immutable = old_observation.observation_revision() == old_revision
            && old_observation.snapshot_cut_ref() == old_cut
            && !old_observation.states().contains(State::Checked);

        assert_eq!(
            reconciled.origin(),
            AtspiObservationOrigin::DirectReconciliation
        );
        assert!(provider_reconciled_checked);
        assert!(fresh_observation_revision_greater_than_old);
        assert_ne!(reconciled.snapshot_cut_ref(), old_cut);
        assert!(old_observation_immutable);

        let permit = provider
            .authorize_action(&binding)
            .await
            .expect("shipping action authorization must perform a fresh direct reconciliation");
        let shipping_permit_has_observation_revision = permit.observation_revision().is_some()
            && permit.observation_cut_ref().is_some();
        assert!(shipping_permit_has_observation_revision);

        write_evidence(&json!({
            "schema": "localview.v43.l04.real-provider.v1",
            "case_id": "L04",
            "candidate_sha": candidate_sha,
            "provider_family": "linux_atspi",
            "ground_truth_source": "real_gtk_atk_atspi_state_and_event_stream",
            "event_assurance": "incomplete",
            "positive_control_state_event_observed": positive_control_state_event_observed,
            "checked_event_observed_after_silent_mutation": checked_event_observed,
            "initial_checked": initial_checked,
            "independent_direct_checked": independent_direct_checked,
            "provider_reconciled_checked": provider_reconciled_checked,
            "fresh_observation_revision_greater_than_old": fresh_observation_revision_greater_than_old,
            "old_observation_immutable": old_observation_immutable,
            "shipping_authorization_succeeded": true,
            "shipping_permit_has_observation_revision": shipping_permit_has_observation_revision
        }));

        observer
            .deregister_event::<ObjectEvents>()
            .await
            .expect("deregister L04 object-event listener");
        let quitting = seed.command("quit");
        assert_eq!(quitting["event"], "quitting");
    }
}
