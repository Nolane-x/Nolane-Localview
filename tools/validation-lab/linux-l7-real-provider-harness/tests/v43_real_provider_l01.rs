#![cfg(target_os = "linux")]

mod linux_real_provider_l01 {
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
        AtspiActionEligibilityError, AtspiBindingLifecycle, AtspiEndpoint, LinuxAtspiProvider,
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
            let seed_bin = std::env::var("LOCALVIEW_L01_SEED_BIN")
                .expect("LOCALVIEW_L01_SEED_BIN must point to the compiled real GTK3 seed");
            let mut child = Command::new(seed_bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch real GTK3 L01 seed");
            let stdout = BufReader::new(child.stdout.take().expect("seed stdout"));
            let mut seed = Self { child, stdout };
            let ready = seed.read_event();
            assert_eq!(ready["event"], "ready", "GTK L01 seed must announce readiness");
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

    fn press_count(seed: &mut SeedProcess) -> u64 {
        let status = seed.command("status");
        assert_eq!(status["event"], "status");
        status["press_count"]
            .as_u64()
            .expect("press_count must be an unsigned integer")
    }

    fn write_evidence(payload: &Value) {
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("LOCALVIEW_L7_ARTIFACT_DIR must be set by the real-provider workflow"),
        );
        fs::create_dir_all(&artifact_dir).expect("create L01 artifact directory");
        fs::write(
            artifact_dir.join("l01-real-provider-evidence.json"),
            serde_json::to_vec_pretty(payload).expect("serialize L01 evidence"),
        )
        .expect("write L01 evidence");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real Ubuntu GTK/ATK/AT-SPI seed"]
    async fn l01_real_defunct_terminally_invalidates_old_binding() {
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("LOCALVIEW_CANDIDATE_SHA must bind evidence to the exact candidate");
        let mut seed = SeedProcess::launch();

        let observer = AccessibilityConnection::new()
            .await
            .expect("connect real AT-SPI observer");
        let object = find_accessible_by_name(&observer, ACCESSIBLE_NAME).await;
        let bus_name = object
            .name_as_str()
            .expect("real accessible must have a unique bus name")
            .to_owned();
        let object_path = object.path_as_str().to_owned();
        let old_proxy = proxy_for(observer.connection(), &object)
            .await
            .expect("build old accessible proxy");

        let initial_states = old_proxy.get_state().await.expect("read initial AT-SPI state");
        let initial_defunct_observed = initial_states.contains(State::Defunct);
        assert!(
            !initial_defunct_observed,
            "live seed must not begin in DEFUNCT state"
        );

        let provider = LinuxAtspiProvider::connect(
            ProviderIncarnationRef::from("provider:linux-atspi:real:l01"),
            TargetIncarnationRef::from("target:linux-atspi:real:l01"),
        )
        .await
        .expect("connect shipping Linux AT-SPI provider");
        let endpoint = AtspiEndpoint::new(bus_name.clone(), object_path.clone());
        let old_binding = provider
            .bind_initial(endpoint.clone(), "cut:l01:real:old")
            .expect("initial real L01 binding must be unique");
        let old_revision = old_binding.binding_revision();

        provider
            .authorize_action(&old_binding)
            .await
            .expect("shipping provider must authorize the live non-DEFUNCT binding");

        let action = ActionProxy::builder(observer.connection())
            .destination(bus_name.clone())
            .expect("valid AT-SPI action destination")
            .path(object_path.clone())
            .expect("valid AT-SPI action path")
            .build()
            .await
            .expect("build real AT-SPI Action proxy");
        assert!(
            action.do_action(0).await.expect("invoke real AT-SPI action 0"),
            "real GTK button must report action success"
        );

        let action_deadline = Instant::now() + Duration::from_secs(2);
        let pre_defunct_press_count = loop {
            let count = press_count(&mut seed);
            if count == 1 {
                break count;
            }
            assert!(
                Instant::now() < action_deadline,
                "real AT-SPI action did not reach GTK seed exactly once"
            );
            sleep(Duration::from_millis(50)).await;
        };

        let destroyed = seed.command("destroy");
        assert_eq!(destroyed["event"], "destroyed");

        let defunct_deadline = Instant::now() + Duration::from_secs(5);
        let final_defunct_observed = loop {
            match old_proxy.get_state().await {
                Ok(states) if states.contains(State::Defunct) => break true,
                Ok(_) => {}
                Err(error) => panic!(
                    "old AT-SPI accessible became unavailable before exposing typed DEFUNCT: {error}"
                ),
            }
            assert!(
                Instant::now() < defunct_deadline,
                "old real AT-SPI accessible never exposed State::Defunct"
            );
            sleep(Duration::from_millis(50)).await;
        };

        let defunct_denial = provider.authorize_action(&old_binding).await;
        assert_eq!(defunct_denial, Err(AtspiActionEligibilityError::Defunct));
        assert_eq!(old_binding.lifecycle(), AtspiBindingLifecycle::InvalidDefunct);

        let post_defunct_press_count = press_count(&mut seed);
        let post_defunct_dispatch_delta = post_defunct_press_count
            .checked_sub(pre_defunct_press_count)
            .expect("post-defunct press count cannot move backwards");
        assert_eq!(post_defunct_dispatch_delta, 0);

        let terminal_denial = provider.authorize_action(&old_binding).await;
        assert_eq!(
            terminal_denial,
            Err(AtspiActionEligibilityError::AlreadyInvalidDefunct)
        );

        let fresh_binding = provider
            .reacquire_after_defunct(&old_binding, endpoint, "cut:l01:real:fresh")
            .expect("L01 fresh binding must be derived from explicit DEFUNCT");
        let fresh_binding_revision_greater_than_old =
            fresh_binding.binding_revision() > old_revision;
        assert!(fresh_binding_revision_greater_than_old);

        write_evidence(&json!({
            "schema": "localview.v43.l01.real-provider.v1",
            "case_id": "L01",
            "candidate_sha": candidate_sha,
            "provider_family": "linux_atspi",
            "initial_defunct_observed": initial_defunct_observed,
            "final_defunct_observed": final_defunct_observed,
            "live_authorization_succeeded": true,
            "pre_defunct_press_count": pre_defunct_press_count,
            "post_defunct_press_count": post_defunct_press_count,
            "post_defunct_dispatch_delta": post_defunct_dispatch_delta,
            "defunct_denial": "defunct",
            "old_binding_terminal_denial": "already_invalid_defunct",
            "old_binding_revival_succeeded": false,
            "fresh_binding_revision_greater_than_old": fresh_binding_revision_greater_than_old,
            "ground_truth_source": "real_gtk_atk_atspi_state"
        }));

        let quitting = seed.command("quit");
        assert_eq!(quitting["event"], "quitting");
    }
}
