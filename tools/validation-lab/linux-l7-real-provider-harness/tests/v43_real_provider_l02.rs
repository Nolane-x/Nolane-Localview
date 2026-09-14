#![cfg(target_os = "linux")]

mod linux_real_provider_l02 {
    use std::{
        collections::{HashSet, VecDeque},
        fs,
        io::{BufRead, BufReader, Write},
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::Duration,
    };

    use atspi::{
        AccessibilityConnection, ObjectRefOwned, State,
        proxy::{accessible::AccessibleProxy, action::ActionProxy},
    };
    use localview_linux_atspi_provider::{
        AtspiActionEligibilityError, AtspiBindingLifecycle, AtspiEndpoint, AtspiReacquireError,
        LinuxAtspiProvider,
    };
    use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
    use serde_json::{Value, json};
    use tokio::time::{Instant, sleep};

    const ACCESSIBLE_NAME: &str = "LocalView L01 Defunct Button";

    struct SeedProcess {
        child: Child,
        stdout: BufReader<std::process::ChildStdout>,
    }

    impl SeedProcess {
        fn launch() -> Self {
            let seed_bin = std::env::var("LOCALVIEW_L02_SEED_BIN")
                .expect("LOCALVIEW_L02_SEED_BIN must point to the compiled real GTK3 seed");
            let mut child = Command::new(seed_bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch real GTK3 L02 seed");
            let stdout = BufReader::new(child.stdout.take().expect("seed stdout"));
            let mut seed = Self { child, stdout };
            let ready = seed.read_event();
            assert_eq!(ready["event"], "ready", "GTK L02 seed must announce readiness");
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
        AccessibleProxy::builder(connection)
            .destination(object.name_as_str()?.to_owned())
            .ok()?
            .path(object.path_as_str().to_owned())
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
                "timed out finding {expected_name:?} through real AT-SPI"
            );
            sleep(Duration::from_millis(100)).await;
        }
    }

    fn press_count(seed: &mut SeedProcess) -> u64 {
        let status = seed.command("status");
        assert_eq!(status["event"], "status");
        status["press_count"].as_u64().expect("press_count")
    }

    fn counts(seed: &mut SeedProcess) -> (u64, u64, u64) {
        let status = seed.command("status");
        assert_eq!(status["event"], "status");
        let total = status["press_count"].as_u64().expect("press_count");
        let original = status["original_press_count"]
            .as_u64()
            .expect("original_press_count");
        let replacement = status["replacement_press_count"]
            .as_u64()
            .expect("replacement_press_count");
        (total, original, replacement)
    }

    fn write_evidence(payload: &Value) {
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("LOCALVIEW_L7_ARTIFACT_DIR must be set"),
        );
        fs::create_dir_all(&artifact_dir).expect("create L02 artifact directory");
        fs::write(
            artifact_dir.join("l02-real-provider-evidence.json"),
            serde_json::to_vec_pretty(payload).expect("serialize L02 evidence"),
        )
        .expect("write L02 evidence");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real Ubuntu GTK/ATK/AT-SPI seed with recreation control"]
    async fn l02_defunct_recreate_never_resurrects_old_binding() {
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("LOCALVIEW_CANDIDATE_SHA must bind evidence to exact candidate");
        let mut seed = SeedProcess::launch();
        let observer = AccessibilityConnection::new()
            .await
            .expect("connect real AT-SPI observer");
        let object = find_accessible_by_name(&observer, ACCESSIBLE_NAME).await;
        let bus_name = object
            .name_as_str()
            .expect("accessible bus name")
            .to_owned();
        let object_path = object.path_as_str().to_owned();
        let proxy = proxy_for(observer.connection(), &object)
            .await
            .expect("build retained real accessible proxy");

        assert!(!proxy
            .get_state()
            .await
            .expect("initial real AT-SPI state")
            .contains(State::Defunct));

        let provider = LinuxAtspiProvider::connect(
            ProviderIncarnationRef::from("provider:linux-atspi:real:l02"),
            TargetIncarnationRef::from("target:linux-atspi:real:l02"),
        )
        .await
        .expect("connect shipping Linux AT-SPI provider");
        let endpoint = AtspiEndpoint::new(bus_name.clone(), object_path.clone());
        let old = provider
            .bind_initial(endpoint.clone(), "cut:l02:real:old")
            .expect("initial L02 binding");
        let old_revision = old.binding_revision();
        provider
            .authorize_action(&old)
            .await
            .expect("initial live binding must authorize");

        let action = ActionProxy::builder(observer.connection())
            .destination(bus_name.clone())
            .expect("action destination")
            .path(object_path.clone())
            .expect("action path")
            .build()
            .await
            .expect("build real action proxy");
        assert!(action.do_action(0).await.expect("initial real action"));

        let initial_action_deadline = Instant::now() + Duration::from_secs(2);
        let before_destroy_total = loop {
            let total = press_count(&mut seed);
            if total == 1 {
                break total;
            }
            assert!(Instant::now() < initial_action_deadline, "initial action count mismatch");
            sleep(Duration::from_millis(50)).await;
        };

        assert_eq!(seed.command("destroy")["event"], "destroyed");
        let defunct_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let states = proxy
                .get_state()
                .await
                .expect("old transport must remain queryable while DEFUNCT");
            if states.contains(State::Defunct) {
                break;
            }
            assert!(Instant::now() < defunct_deadline, "typed DEFUNCT never appeared");
            sleep(Duration::from_millis(50)).await;
        }

        assert_eq!(
            provider.authorize_action(&old).await,
            Err(AtspiActionEligibilityError::Defunct)
        );
        assert_eq!(old.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);

        let recreated = seed.command("recreate");
        assert_eq!(
            recreated["event"], "recreated",
            "real seed must recreate a live backing widget behind the retained AT-SPI endpoint"
        );

        let (after_recreate_before_action_total, before_destroy_original, before_destroy_replacement) =
            counts(&mut seed);
        assert_eq!(after_recreate_before_action_total, before_destroy_total);
        assert_eq!(before_destroy_original, 1);
        assert_eq!(before_destroy_replacement, 0);

        let live_again_deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let states = proxy
                .get_state()
                .await
                .expect("same retained AT-SPI transport must remain queryable after recreation");
            if !states.contains(State::Defunct) {
                break;
            }
            assert!(Instant::now() < live_again_deadline, "recreated endpoint stayed DEFUNCT");
            sleep(Duration::from_millis(50)).await;
        }

        assert_eq!(
            provider.authorize_action(&old).await,
            Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
        );

        let fresh = provider
            .reacquire_after_defunct(&old, endpoint, "cut:l02:real:fresh")
            .expect("explicit DEFUNCT must permit one fresh authority transition");
        assert!(fresh.binding_revision() > old_revision);
        provider
            .authorize_action(&fresh)
            .await
            .expect("fresh binding must authorize against recreated live endpoint");

        assert!(action.do_action(0).await.expect("replacement real action"));
        let replacement_deadline = Instant::now() + Duration::from_secs(2);
        let (after_total, after_original, after_replacement) = loop {
            let values = counts(&mut seed);
            if values == (2, 1, 1) {
                break values;
            }
            assert!(Instant::now() < replacement_deadline, "replacement action count mismatch");
            sleep(Duration::from_millis(50)).await;
        };

        assert_eq!(
            provider
                .reacquire_after_defunct(
                    &old,
                    fresh.endpoint().clone(),
                    "cut:l02:real:replay",
                )
                .expect_err("old DEFUNCT authority must be single-use"),
            AtspiReacquireError::PreviousBindingSuperseded
        );

        write_evidence(&json!({
            "schema": "localview.v43.l02.real-provider.v1",
            "case_id": "L02",
            "candidate_sha": candidate_sha,
            "provider_family": "linux_atspi",
            "same_endpoint_reused": true,
            "typed_defunct_observed": true,
            "old_binding_terminal_denial": "already_invalid_defunct",
            "old_defunct_authority_replay_denial": "previous_binding_superseded",
            "fresh_binding_revision_greater_than_old": fresh.binding_revision() > old_revision,
            "fresh_authorization_succeeded": true,
            "before_destroy_total": before_destroy_total,
            "before_destroy_original": before_destroy_original,
            "before_destroy_replacement": before_destroy_replacement,
            "after_recreate_total": after_total,
            "after_recreate_original": after_original,
            "after_recreate_replacement": after_replacement,
            "ground_truth_source": "real_gtk_atk_atspi_state_plus_seed_effect_counters"
        }));

        assert_eq!(seed.command("quit")["event"], "quitting");
    }
}
