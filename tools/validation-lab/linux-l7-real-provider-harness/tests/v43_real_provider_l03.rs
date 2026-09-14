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
            assert_eq!(ready["blocker_present"], true, "L03 seed must begin occluded");
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

    async fn component_for<'a>(
        connection: &'a atspi::zbus::Connection,
        object: &ObjectRefOwned,
    ) -> ComponentProxy<'a> {
        ComponentProxy::builder(connection)
            .destination(object.name_as_str().expect("component bus name").to_owned())
            .expect("component destination")
            .path(object.path_as_str().to_owned())
            .expect("component path")
            .build()
            .await
            .expect("build component proxy")
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

    async fn parent_of(
        connection: &AccessibilityConnection,
        object: &ObjectRefOwned,
    ) -> ObjectRefOwned {
        let proxy = proxy_for(connection.connection(), object)
            .await
            .expect("build accessible proxy for parent lookup");
        let parent = proxy.parent().await.expect("read accessible parent");
        assert!(!parent.is_null(), "accessible must have a non-null parent");
        parent
    }

    fn same_object(left: &ObjectRefOwned, right: &ObjectRefOwned) -> bool {
        left.name_as_str() == right.name_as_str() && left.path_as_str() == right.path_as_str()
    }

    async fn extents_of(
        connection: &AccessibilityConnection,
        object: &ObjectRefOwned,
    ) -> (i32, i32, i32, i32) {
        component_for(connection.connection(), object)
            .await
            .get_extents(CoordType::Screen)
            .await
            .expect("read real AT-SPI component extents")
    }

    fn center_of(rect: (i32, i32, i32, i32)) -> (i32, i32) {
        let (x, y, width, height) = rect;
        assert!(width > 0 && height > 0, "accessible must occupy a positive screen rectangle");
        (
            x.checked_add(width / 2).expect("center x must not overflow"),
            y.checked_add(height / 2).expect("center y must not overflow"),
        )
    }

    fn rect_contains(rect: (i32, i32, i32, i32), point: (i32, i32)) -> bool {
        let (x, y, width, height) = rect;
        if width <= 0 || height <= 0 {
            return false;
        }
        let left = i64::from(x);
        let top = i64::from(y);
        let right = left + i64::from(width);
        let bottom = top + i64::from(height);
        let px = i64::from(point.0);
        let py = i64::from(point.1);
        px >= left && px < right && py >= top && py < bottom
    }

    async fn hit_name_at_point(
        connection: &AccessibilityConnection,
        parent: &ObjectRefOwned,
        point: (i32, i32),
    ) -> String {
        let hit = component_for(connection.connection(), parent)
            .await
            .get_accessible_at_point(point.0, point.1, CoordType::Screen)
            .await
            .expect("real parent hit-test at target center");
        assert!(!hit.is_null(), "real AT-SPI hit-test must resolve an accessible");
        proxy_for(connection.connection(), &hit)
            .await
            .expect("build hit accessible proxy")
            .name()
            .await
            .expect("read hit accessible name")
    }

    fn x11_click(point: (i32, i32)) {
        let move_status = Command::new("xdotool")
            .arg("mousemove")
            .arg("--sync")
            .arg(point.0.to_string())
            .arg(point.1.to_string())
            .status()
            .expect("run xdotool mousemove");
        assert!(move_status.success(), "xdotool mousemove must succeed");

        let click_status = Command::new("xdotool")
            .arg("click")
            .arg("1")
            .status()
            .expect("run xdotool click");
        assert!(click_status.success(), "xdotool click must succeed");
    }

    async fn wait_for_press_counts(
        seed: &mut SeedProcess,
        expected_target: u64,
        expected_blocker: u64,
        phase: &str,
    ) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let status = seed.command("status");
            let target = status["target_press_count"]
                .as_u64()
                .expect("target_press_count must be numeric");
            let blocker = status["blocker_press_count"]
                .as_u64()
                .expect("blocker_press_count must be numeric");
            if target == expected_target && blocker == expected_blocker {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for pointer recipient counts during {phase}: target={target}, blocker={blocker}"
            );
            sleep(Duration::from_millis(50)).await;
        }
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
        let blocker_object = find_accessible_by_name(&observer, BLOCKER_NAME).await;
        let target_proxy = proxy_for(observer.connection(), &target_object)
            .await
            .expect("build real target accessible proxy");
        let blocker_proxy = proxy_for(observer.connection(), &blocker_object)
            .await
            .expect("build real blocker accessible proxy");

        let target_states = target_proxy
            .get_state()
            .await
            .expect("read real target state while occluded");
        assert!(target_states.contains(State::Visible), "occluded target must remain VISIBLE");
        assert!(target_states.contains(State::Showing), "occluded target must remain SHOWING");

        let blocker_states = blocker_proxy
            .get_state()
            .await
            .expect("read real blocker state");
        assert!(blocker_states.contains(State::Visible), "real blocker must be VISIBLE");
        assert!(blocker_states.contains(State::Showing), "real blocker must be SHOWING");

        let target_parent = parent_of(&observer, &target_object).await;
        let blocker_parent = parent_of(&observer, &blocker_object).await;
        assert!(
            same_object(&target_parent, &blocker_parent),
            "target and blocker must be siblings in the real AT-SPI tree"
        );

        let target_rect = extents_of(&observer, &target_object).await;
        let blocker_rect = extents_of(&observer, &blocker_object).await;
        let target_center = center_of(target_rect);
        assert!(
            rect_contains(blocker_rect, target_center),
            "real blocker extents must cover the target center"
        );

        let raw_atspi_hit_before =
            hit_name_at_point(&observer, &target_parent, target_center).await;
        assert_eq!(
            raw_atspi_hit_before, TARGET_NAME,
            "GTK3 baseline must reproduce the AT-SPI self-hit false positive"
        );

        x11_click(target_center);
        let blocked_status = wait_for_press_counts(&mut seed, 0, 1, "occluded click").await;
        assert_eq!(blocked_status["blocker_present"], true);

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
            "shipping pointer authority must reject a target self-hit when a visible/showing sibling covers the tested point"
        );

        let unblock = seed.command("unblock");
        assert_eq!(unblock["event"], "unblocked");
        let clear_deadline = Instant::now() + Duration::from_secs(5);
        let raw_atspi_hit_after = loop {
            let name = hit_name_at_point(&observer, &target_parent, target_center).await;
            if name == TARGET_NAME {
                break name;
            }
            assert!(
                Instant::now() < clear_deadline,
                "real AT-SPI hit-test did not settle on target after blocker removal"
            );
            sleep(Duration::from_millis(50)).await;
        };

        x11_click(target_center);
        let clear_status = wait_for_press_counts(&mut seed, 1, 1, "unblocked click").await;
        assert_eq!(clear_status["blocker_present"], false);

        let permit = provider
            .authorize_pointer_action(&binding)
            .await
            .expect("shipping pointer authority must reopen after the real blocker is removed");
        assert_eq!(permit.binding_revision(), binding.binding_revision());

        write_evidence(&json!({
            "schema": "localview.v43.l03.real-provider.v2",
            "case_id": "L03",
            "candidate_sha": candidate_sha,
            "provider_family": "linux_atspi",
            "target_visible_while_occluded": true,
            "target_showing_while_occluded": true,
            "blocker_visible_while_occluding": true,
            "blocker_showing_while_occluding": true,
            "blocker_overlaps_target_center": true,
            "target_and_blocker_are_atspi_siblings": true,
            "raw_atspi_hit_before": raw_atspi_hit_before,
            "atspi_self_hit_false_positive": true,
            "actual_pointer_recipient_before": BLOCKER_NAME,
            "shipping_pointer_denial": "occluded",
            "semantic_authorization_while_occluded": true,
            "raw_atspi_hit_after": raw_atspi_hit_after,
            "actual_pointer_recipient_after": TARGET_NAME,
            "post_unblock_pointer_authorized": true,
            "ground_truth_source": "real_x11_pointer_delivery_plus_gtk_atk_atspi_geometry"
        }));

        assert_eq!(seed.command("quit")["event"], "quitting");
    }
}
