#![cfg(target_os = "linux")]

mod linux_real_provider_l05 {
    use std::{
        collections::{HashSet, VecDeque},
        fs,
        io::{BufRead, BufReader, Write},
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::Duration,
    };

    use atspi::{
        AccessibilityConnection, ObjectRefOwned,
        proxy::{accessible::AccessibleProxy, action::ActionProxy},
    };
    use localview_linux_atspi_provider::{
        AtspiAccessibilityBusLifecycle, AtspiActionEligibilityError, AtspiEndpoint,
        LinuxAtspiProvider,
    };
    use localview_protocol::{ProviderIncarnationRef, TargetIncarnationRef};
    use serde_json::{Value, json};
    use tokio::time::{Instant, sleep};

    const ACCESSIBLE_NAME: &str = "LocalView L05 Bus Reconnect Button";

    struct SeedProcess {
        child: Child,
        stdout: BufReader<std::process::ChildStdout>,
    }

    impl SeedProcess {
        fn launch() -> Self {
            let seed_bin = std::env::var("LOCALVIEW_L05_SEED_BIN")
                .expect("LOCALVIEW_L05_SEED_BIN must point to the compiled real GTK3 seed");
            let mut child = Command::new(seed_bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("launch real GTK3 L05 seed");
            let stdout = BufReader::new(child.stdout.take().expect("seed stdout"));
            let mut seed = Self { child, stdout };
            let ready = seed.read_event();
            assert_eq!(ready["event"], "ready", "GTK L05 seed must announce readiness");
            seed
        }

        fn pid(&self) -> u32 {
            self.child.id()
        }

        fn is_running(&mut self) -> bool {
            self.child
                .try_wait()
                .expect("inspect GTK L05 seed process")
                .is_none()
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
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(root) = connection.root_accessible_on_registry().await {
                if let Ok(children) = root.get_children().await {
                    let mut queue: VecDeque<ObjectRefOwned> = children.into();
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
                }
            }

            assert!(
                Instant::now() < deadline,
                "timed out finding {expected_name:?} through the real AT-SPI registry"
            );
            sleep(Duration::from_millis(100)).await;
        }
    }

    async fn connect_replacement_observer() -> AccessibilityConnection {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(connection) = AccessibilityConnection::new().await {
                if connection.root_accessible_on_registry().await.is_ok() {
                    return connection;
                }
            }
            assert!(
                Instant::now() < deadline,
                "replacement AT-SPI accessibility bus never became connectable"
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

    async fn wait_for_press_count(
        seed: &mut SeedProcess,
        expected: u64,
        failure_message: &str,
    ) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let count = press_count(seed);
            if count == expected {
                return count;
            }
            assert!(Instant::now() < deadline, "{failure_message}: expected={expected} actual={count}");
            sleep(Duration::from_millis(50)).await;
        }
    }

    fn terminate_real_accessibility_bus_child() -> String {
        let script = r#"
set -euo pipefail
launcher="$(ps -eo pid=,ppid=,args= | awk '/[a]t-spi-bus-launcher/ {print $1; exit}')"
if [ -z "$launcher" ]; then
  echo 'no at-spi-bus-launcher found' >&2
  exit 20
fi
child="$(ps -eo pid=,ppid=,args= | awk -v p="$launcher" '$2 == p && ($0 ~ /[d]bus-daemon/ || $0 ~ /[d]bus-broker/) {print $1; exit}')"
if [ -z "$child" ]; then
  echo "no accessibility bus child found under launcher=$launcher" >&2
  ps -eo pid=,ppid=,args= >&2
  exit 21
fi
kill -TERM "$child"
for _ in $(seq 1 100); do
  if ! kill -0 "$child" 2>/dev/null; then
    break
  fi
  sleep 0.05
done
if kill -0 "$child" 2>/dev/null; then
  echo "accessibility bus child did not exit: $child" >&2
  exit 22
fi
printf '%s:%s\n' "$launcher" "$child"
"#;
        let output = Command::new("bash")
            .arg("-lc")
            .arg(script)
            .output()
            .expect("run accessibility-bus restart probe");
        assert!(
            output.status.success(),
            "failed to terminate real accessibility bus child: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("restart probe output must be UTF-8")
            .trim()
            .to_owned()
    }

    fn killed_bus_child_pid(killed_processes: &str) -> u32 {
        killed_processes
            .split_once(':')
            .expect("killed process evidence must be launcher:child")
            .1
            .parse()
            .expect("killed accessibility bus child PID must be numeric")
    }

    fn current_accessibility_bus_child_pid() -> u32 {
        let script = r#"
set -euo pipefail
launcher="$(ps -eo pid=,ppid=,args= | awk '/[a]t-spi-bus-launcher/ {print $1; exit}')"
[ -n "$launcher" ]
child="$(ps -eo pid=,ppid=,args= | awk -v p="$launcher" '$2 == p && ($0 ~ /[d]bus-daemon/ || $0 ~ /[d]bus-broker/) {print $1; exit}')"
[ -n "$child" ]
printf '%s\n' "$child"
"#;
        let output = Command::new("bash")
            .arg("-lc")
            .arg(script)
            .output()
            .expect("inspect replacement accessibility bus child");
        assert!(
            output.status.success(),
            "replacement accessibility bus child not found: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("replacement child PID must be UTF-8")
            .trim()
            .parse()
            .expect("replacement accessibility bus child PID must be numeric")
    }

    async fn wait_until_old_bus_is_dead(connection: &AccessibilityConnection) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if connection.root_accessible_on_registry().await.is_err() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    fn write_evidence(payload: &Value) {
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("LOCALVIEW_L7_ARTIFACT_DIR must be set by the real-provider workflow"),
        );
        fs::create_dir_all(&artifact_dir).expect("create L05 artifact directory");
        fs::write(
            artifact_dir.join("l05-real-provider-evidence.json"),
            serde_json::to_vec_pretty(payload).expect("serialize L05 evidence"),
        )
        .expect("write L05 evidence");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a real Ubuntu GTK/ATK/AT-SPI accessibility-bus restart"]
    async fn l05_real_accessibility_bus_reconnect_invalidates_old_binding() {
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("LOCALVIEW_CANDIDATE_SHA must bind evidence to the exact candidate");
        let mut seed = SeedProcess::launch();
        let target_pid = seed.pid();

        let old_observer = AccessibilityConnection::new()
            .await
            .expect("connect independent observer to initial accessibility bus");
        let old_object = find_accessible_by_name(&old_observer, ACCESSIBLE_NAME).await;
        let old_bus_name = old_object
            .name_as_str()
            .expect("real accessible must have a unique bus name")
            .to_owned();
        let old_object_path = old_object.path_as_str().to_owned();
        let old_proxy = proxy_for(old_observer.connection(), &old_object)
            .await
            .expect("build old real accessible proxy");
        old_proxy.get_state().await.expect("old bus state query before restart");

        let mut provider = LinuxAtspiProvider::connect(
            ProviderIncarnationRef::from("provider:linux-atspi:real:l05"),
            TargetIncarnationRef::from("target:linux-atspi:real:l05"),
        )
        .await
        .expect("connect shipping Linux AT-SPI provider to initial bus");
        let old_binding = provider
            .bind_initial(
                AtspiEndpoint::new(old_bus_name.clone(), old_object_path.clone()),
                "cut:l05:real:old",
            )
            .expect("initial real L05 binding");
        let old_binding_revision = old_binding.binding_revision();
        let old_bus_incarnation = *old_binding.accessibility_bus_incarnation_ref();
        provider
            .authorize_action(&old_binding)
            .await
            .expect("old binding must be actionable before the real bus restart");

        let old_action = ActionProxy::builder(old_observer.connection())
            .destination(old_bus_name.clone())
            .expect("valid old AT-SPI action destination")
            .path(old_object_path.clone())
            .expect("valid old AT-SPI action path")
            .build()
            .await
            .expect("build old real AT-SPI Action proxy");
        let pre_restart_before = press_count(&mut seed);
        assert!(
            old_action
                .do_action(0)
                .await
                .expect("invoke real AT-SPI action before reconnect"),
            "old real GTK button must report action success before restart"
        );
        let pre_restart_after = wait_for_press_count(
            &mut seed,
            pre_restart_before + 1,
            "pre-restart AT-SPI action did not reach GTK seed exactly once",
        )
        .await;
        let pre_restart_action_delta = pre_restart_after - pre_restart_before;
        assert_eq!(pre_restart_action_delta, 1);

        let killed_processes = terminate_real_accessibility_bus_child();
        let old_bus_child_pid = killed_bus_child_pid(&killed_processes);
        let old_transport_unavailable = wait_until_old_bus_is_dead(&old_observer).await;
        assert!(
            old_transport_unavailable,
            "old AT-SPI connection must become unusable after killing the real accessibility bus"
        );

        let old_binding_denied_while_disconnected = matches!(
            provider.authorize_action(&old_binding).await,
            Err(AtspiActionEligibilityError::AccessibilityBusDisconnected)
        );
        assert!(
            old_binding_denied_while_disconnected,
            "shipping provider must detect the closed AT-SPI transport and fence the old binding without a manual disconnect hint"
        );
        assert_eq!(
            provider.accessibility_bus_lifecycle(),
            AtspiAccessibilityBusLifecycle::Disconnected
        );

        let target_process_survived_bus_restart = seed.is_running()
            && seed.pid() == target_pid
            && press_count(&mut seed) == pre_restart_after;
        assert!(
            target_process_survived_bus_restart,
            "GTK target process must survive the accessibility-bus restart unchanged and receive no phantom action"
        );

        let fresh_observer = connect_replacement_observer().await;
        let fresh_object = find_accessible_by_name(&fresh_observer, ACCESSIBLE_NAME).await;
        let replacement_bus_child_pid = current_accessibility_bus_child_pid();
        assert_ne!(
            replacement_bus_child_pid, old_bus_child_pid,
            "real accessibility bus child PID must change across L05 reincarnation"
        );

        let fresh_bus_incarnation = provider
            .reconnect_accessibility_bus()
            .await
            .expect("shipping provider must reconnect through org.a11y.Bus to a fresh bus");
        assert_ne!(fresh_bus_incarnation, old_bus_incarnation);
        assert_eq!(
            provider.accessibility_bus_lifecycle(),
            AtspiAccessibilityBusLifecycle::Connected
        );
        assert_eq!(
            provider.authorize_action(&old_binding).await,
            Err(AtspiActionEligibilityError::AccessibilityBusIncarnationMismatch)
        );

        let fresh_bus_name = fresh_object
            .name_as_str()
            .expect("fresh real accessible must have a unique bus name")
            .to_owned();
        let fresh_object_path = fresh_object.path_as_str().to_owned();
        let endpoint_reused = fresh_bus_name == old_bus_name && fresh_object_path == old_object_path;

        let fresh_binding = provider
            .reacquire_after_bus_reconnect(
                &old_binding,
                AtspiEndpoint::new(fresh_bus_name.clone(), fresh_object_path.clone()),
                "cut:l05:real:fresh",
            )
            .expect("explicit real L05 reacquire on replacement bus");
        let fresh_binding_revision_greater_than_old =
            fresh_binding.binding_revision() > old_binding_revision;
        assert!(fresh_binding_revision_greater_than_old);
        assert_eq!(
            *fresh_binding.accessibility_bus_incarnation_ref(),
            fresh_bus_incarnation
        );
        let fresh_permit = provider
            .authorize_action(&fresh_binding)
            .await
            .expect("fresh binding on replacement bus must authorize");
        assert_eq!(
            fresh_permit.accessibility_bus_incarnation_ref(),
            fresh_binding.accessibility_bus_incarnation_ref()
        );

        let action = ActionProxy::builder(fresh_observer.connection())
            .destination(fresh_bus_name)
            .expect("valid fresh AT-SPI action destination")
            .path(fresh_object_path)
            .expect("valid fresh AT-SPI action path")
            .build()
            .await
            .expect("build fresh real AT-SPI Action proxy");
        let before_press_count = press_count(&mut seed);
        assert_eq!(
            before_press_count, pre_restart_after,
            "bus reincarnation must not dispatch an action as a side effect"
        );
        assert!(
            action.do_action(0).await.expect("invoke real AT-SPI action after reconnect"),
            "fresh real GTK button must report action success"
        );
        let after_press_count = wait_for_press_count(
            &mut seed,
            before_press_count + 1,
            "fresh AT-SPI action did not reach the original GTK process after bus reconnect",
        )
        .await;

        write_evidence(&json!({
            "schema": "localview.v43.l05.real-provider.v1",
            "case_id": "L05",
            "candidate_sha": candidate_sha,
            "provider_family": "linux_atspi",
            "ground_truth_source": "real_atspi_bus_process_restart_plus_gtk_action_effect",
            "pre_restart_action_delta": pre_restart_action_delta,
            "old_transport_unavailable": old_transport_unavailable,
            "old_binding_denied_while_disconnected": old_binding_denied_while_disconnected,
            "bus_incarnation_changed": fresh_bus_incarnation != old_bus_incarnation,
            "old_binding_denied_after_reconnect": true,
            "fresh_binding_revision_greater_than_old": fresh_binding_revision_greater_than_old,
            "fresh_authorization_succeeded": true,
            "post_reconnect_action_delta": after_press_count - before_press_count,
            "target_process_survived_bus_restart": target_process_survived_bus_restart,
            "target_pid": target_pid,
            "old_bus_child_pid": old_bus_child_pid,
            "replacement_bus_child_pid": replacement_bus_child_pid,
            "endpoint_reused": endpoint_reused,
            "killed_processes": killed_processes
        }));

        let quitting = seed.command("quit");
        assert_eq!(quitting["event"], "quitting");
    }
}
