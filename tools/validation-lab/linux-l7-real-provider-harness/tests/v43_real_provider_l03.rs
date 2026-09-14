#![cfg(target_os = "linux")]

mod linux_real_provider_l03 {
    use std::{
        collections::{HashSet, VecDeque},
        fs,
        io::{BufRead, BufReader, Write},
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::Duration,
    };

    use atspi::{
        AccessibilityConnection, CoordType, ObjectRefOwned, State,
        proxy::{accessible::AccessibleProxy, component::ComponentProxy},
    };
    use localview_linux_atspi_provider::{
        AtspiEndpoint, AtspiPointerEligibilityError, LinuxAtspiProvider,
    };
    use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
    use serde_json::{Value, json};
    use tokio::time::{Instant, sleep};

    const TARGET_NAME: &str = "LocalView L03 Visible Target";
    const BLOCKER_NAME: &str = "LocalView L03 Occluding Blocker";

    struct SeedProcess {
        child: Child,
        stdout: BufReader<std::process::ChildStdout>,
    }

    impl SeedProcess {
        fn launch() -> Self {
            let seed_bin = std::env::var("LOCALVIEW_L03_SEED_BIN")
                .expect("LOCALVIEW_L03_SEED_BIN must point to the compiled real GTK3 seed");
            let mut child = Command::new(seed_bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch real GTK3 L03 seed");
            let stdout = BufReader::new(child.stdout.take().expect("seed stdout"));
            let mut seed = Self { child, stdout };
            let ready = seed.read_event();
            assert_eq!(ready["event"], "ready", "GTK L03 seed must announce readiness");
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

    async fn hit_name_at_target_center(
        connection: &AccessibilityConnection,
        target_object: &ObjectRefOwned,
    ) -> String {
        let bus = connection.connection();
        let target_proxy = proxy_for(bus, target_object)
            .await
            .expect("build target accessible proxy");
        let target_component = ComponentProxy::builder(bus)
            .destination(target_object.name_as_str().expect("target bus name").to_owned())
            .expect("target component destination")
            .path(target_object.path_as_str().to_owned())
            .expect("target component path")
            .build()
            .await
            .expect("build target component proxy");
        let (x, y, width, height) = target_component
            .get_extents(CoordType::Screen)
            .await
            .expect("target extents from real AT-SPI");
        assert!(width > 0 && height > 0, "target must occupy a real screen rectangle");
        let center_x = x.checked_add(width / 2).expect("target center x");
        let center_y = y.checked_add(height / 2).expect("target center y");

        let parent = target_proxy.parent().await.expect("target parent");
        assert!(!parent.is_null(), "target must have an AT-SPI parent");
        let parent_component = ComponentProxy::builder(bus)
            .destination(parent.name_as_str().expect("parent bus name").to_owned())
            .expect("parent component destination")
            .path(parent.path_as_str().to_owned())
            .expect("parent component path")
            .build()
            .await
            .expect("build parent component proxy");
        let hit = parent_component
            .get_accessible_at_point(center_x, center_y, CoordType::Screen)
            .await
            .expect("real parent hit-test at target center");
        assert!(!hit.is_null(), "real hit-test must resolve an accessible");
        proxy_for(bus, &hit)
            .await
            .expect("build hit accessible proxy")
            .name()
            .await
            .expect("read hit accessible name")
    }

    fn write_evidence(payload: &Value) {
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("LOCALVIEW_L7_ARTIFACT_DIR must be set"),
        );
        fs::create_dir_all(&artifact_dir).expect("create L03 artifact directory");
        fs::write(
            artifact_dir.join("l03-real-provider-evidence.json"),
            serde_json::to_vec_pretty(payload).expect("serialize L03 evidence"),
        )
        .expect("write L03 evidence");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real Ubuntu GTK/ATK/AT-SPI visible-occlusion seed"]
    async fn l03_visible_target_requires_real_unobscured_hit_test() {
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("LOCALVIEW_CANDIDATE_SHA must bind evidence to exact candidate");
        let mut seed = SeedProcess::launch();
        let observer = AccessibilityConnection::new()
            .await
            .expect("connect real AT-SPI observer");
        let target_object = find_accessible_by_name(&observer, TARGET_NAME).await;
        let target_proxy = proxy_for(observer.connection(), &target_object)
            .await
            .expect("build real target accessible proxy");
        let states = target_proxy
            .get_state()
            .await
            .expect("read real target state while occluded");
        assert!(states.contains(State::Visible), "occluded target must remain VISIBLE");
        assert!(states.contains(State::Showing), "occluded target must remain SHOWING");

        let raw_hit_before = hit_name_at_target_center(&observer, &target_object).await;
        assert_eq!(
            raw_hit_before, BLOCKER_NAME,
            "real AT-SPI parent hit-test must resolve the blocker, not the visible target"
        );

        let provider = LinuxAtspiProvider::connect(
            ProviderIncarnationRef::from("provider:linux-atspi:real:l03"),
            TargetIncarnationRef::from("target:linux-atspi:real:l03"),
        )
        .await
        .expect("connect shipping Linux AT-SPI provider");
        let endpoint = AtspiEndpoint::new(
            target_object.name_as_str().expect("target bus name").to_owned(),
            target_object.path_as_str().to_owned(),
        );
        let binding = provider
            .bind_initial(endpoint, "cut:l03:real:visible-occluded")
            .expect("bind real L03 target");

        provider
            .authorize_action(&binding)
            .await
            .expect("semantic action eligibility must remain distinct from pointer reachability");
        assert_eq!(
            provider.authorize_pointer_action(&binding).await,
            Err(AtspiPointerEligibilityError::Occluded),
            "shipping pointer authority must reject the real blocker hit"
        );

        let unblock = seed.command("unblock");
        assert_eq!(unblock["event"], "unblocked");
        let clear_deadline = Instant::now() + Duration::from_secs(5);
        let raw_hit_after = loop {
            let name = hit_name_at_target_center(&observer, &target_object).await;
            if name == TARGET_NAME {
                break name;
            }
            assert!(
                Instant::now() < clear_deadline,
                "real AT-SPI hit-test did not return to target after blocker removal"
            );
            sleep(Duration::from_millis(50)).await;
        };

        let permit = provider
            .authorize_pointer_action(&binding)
            .await
            .expect("shipping pointer authority must reopen after the blocker is really removed");
        assert_eq!(permit.binding_revision(), binding.binding_revision());

        write_evidence(&json!({
            "schema": "localview.v43.l03.real-provider.v1",
            "case_id": "L03",
            "candidate_sha": candidate_sha,
            "provider_family": "linux_atspi",
            "target_visible_while_occluded": true,
            "target_showing_while_occluded": true,
            "raw_hit_before": raw_hit_before,
            "shipping_pointer_denial": "occluded",
            "semantic_authorization_while_occluded": true,
            "raw_hit_after": raw_hit_after,
            "post_unblock_pointer_authorized": true,
            "ground_truth_source": "real_gtk_atk_atspi_component_hit_test"
        }));

        assert_eq!(seed.command("quit")["event"], "quitting");
    }
}
