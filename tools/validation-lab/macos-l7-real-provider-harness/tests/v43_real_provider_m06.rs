#[cfg(target_os = "macos")]
mod macos_real_provider_m06 {
    use std::{
        ffi::{c_char, c_void, CString},
        fs,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        ptr,
        thread,
        time::{Duration, Instant},
    };

    use localview_macos_ax_provider::{
        AxApplicationIncarnation, AxObserverApplicationBindingProvider,
        AxObserverApplicationDecision, AxObserverRecreateDirective, AxPermissionProvider,
        AxPermissionState,
    };

    type AxUiElementRef = *const c_void;
    type AxObserverRef = *const c_void;
    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;

    const AX_ERROR_SUCCESS: i32 = 0;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const APPLICATION_IDENTITY: &str = "com.nolane.localview.m06-seed";
    const WINDOW_TITLE: &str = "LocalView M06 Restart Seed";
    const NOTIFICATION: &str = "AXFocusedUIElementChanged";

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXObserverCreate(
            application: i32,
            callback: unsafe extern "C" fn(
                AxObserverRef,
                AxUiElementRef,
                CfStringRef,
                *mut c_void,
            ),
            out_observer: *mut AxObserverRef,
        ) -> i32;
        fn AXObserverAddNotification(
            observer: AxObserverRef,
            element: AxUiElementRef,
            notification: CfStringRef,
            refcon: *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            c_string: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFRelease(value: CfTypeRef);
    }

    unsafe extern "C" fn observer_callback(
        _observer: AxObserverRef,
        _element: AxUiElementRef,
        _notification: CfStringRef,
        _refcon: *mut c_void,
    ) {
        // M06 proves observer/application incarnation only. Callback delivery
        // and run-loop continuity are deliberately reserved for M07.
    }

    struct OwnedCf(CfTypeRef);

    impl OwnedCf {
        fn new(value: CfTypeRef) -> Option<Self> {
            (!value.is_null()).then_some(Self(value))
        }

        fn raw(&self) -> CfTypeRef {
            self.0
        }
    }

    impl Drop for OwnedCf {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    struct SeedProcess(Child);

    impl SeedProcess {
        fn terminate(mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Drop for SeedProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn cf_string(value: &str) -> Result<OwnedCf, String> {
        let value = CString::new(value)
            .map_err(|_| "CoreFoundation string contains interior NUL".to_owned())?;
        let string = unsafe {
            CFStringCreateWithCString(ptr::null(), value.as_ptr(), CF_STRING_ENCODING_UTF8)
        };
        OwnedCf::new(string).ok_or_else(|| "CFStringCreateWithCString returned null".to_owned())
    }

    fn read_state(path: &Path) -> Result<serde_json::Value, String> {
        let bytes = fs::read(path).map_err(|error| format!("read M06 seed state: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("parse M06 seed state: {error}"))
    }

    fn wait_for_seed(path: &Path) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        while Instant::now() < deadline {
            if let Ok(state) = read_state(path) {
                if state["ready"].as_bool() == Some(true) {
                    return Ok(state);
                }
                last = Some(state);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(format!("M06 seed did not become ready; last={last:?}"))
    }

    fn launch_seed(seed_executable: &str, state_path: &Path) -> SeedProcess {
        let _ = fs::remove_file(state_path);
        let child = Command::new(seed_executable)
            .env("LOCALVIEW_M06_STATE_PATH", state_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch exact-head M06 AppKit seed");
        SeedProcess(child)
    }

    fn create_observer(pid: i32) -> (OwnedCf, i32) {
        let mut observer: AxObserverRef = ptr::null();
        let error = unsafe { AXObserverCreate(pid, observer_callback, &mut observer) };
        let observer = OwnedCf::new(observer.cast()).expect("AXObserverCreate returned null observer");
        (observer, error)
    }

    fn register_application_notification(
        observer: &OwnedCf,
        pid: i32,
        notification: &OwnedCf,
    ) -> i32 {
        let application = OwnedCf::new(unsafe { AXUIElementCreateApplication(pid) }.cast())
            .expect("AXUIElementCreateApplication returned null");
        unsafe {
            AXObserverAddNotification(
                observer.raw().cast(),
                application.raw().cast(),
                notification.raw().cast(),
                ptr::null_mut(),
            )
        }
    }

    #[test]
    #[ignore = "requires real macOS AppKit + Accessibility observer behavior"]
    fn m06_relaunch_retires_old_observer_application_incarnation() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );

        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M06 requires LOCALVIEW_CANDIDATE_SHA");
        let seed_executable = std::env::var("LOCALVIEW_M06_SEED_EXECUTABLE")
            .expect("M06 requires exact-head seed executable");
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M06 requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M06 artifact directory");
        let first_state_path = artifact_dir.join("M06-SEED-STATE-1.json");
        let second_state_path = artifact_dir.join("M06-SEED-STATE-2.json");

        let permission = AxPermissionProvider::new().current_permission_revision(false);
        assert_eq!(
            permission.state(),
            AxPermissionState::Trusted,
            "M06 requires a real trusted Accessibility topology"
        );

        let first_seed = launch_seed(&seed_executable, &first_state_path);
        let first_state = wait_for_seed(&first_state_path).expect("wait for first M06 seed");
        assert_eq!(first_state["schema"], "localview-v43-m06-seed-state-v1");
        assert_eq!(first_state["window_title"], WINDOW_TITLE);
        let first_pid = first_state["pid"].as_i64().expect("first M06 pid") as i32;
        let first_launch_marker = first_state["launch_marker"]
            .as_u64()
            .expect("first M06 launch marker");

        let first_incarnation = AxApplicationIncarnation::new(
            APPLICATION_IDENTITY,
            first_pid,
            first_launch_marker,
        );
        let authority = AxObserverApplicationBindingProvider::new();
        let first_binding = authority.bind_current(first_incarnation.clone());
        let first_observer_revision = first_binding.observer_creation_revision();

        let notification = cf_string(NOTIFICATION).expect("build M06 notification");
        let (first_observer, first_observer_create_error) = create_observer(first_pid);
        assert_eq!(
            first_observer_create_error, AX_ERROR_SUCCESS,
            "first application incarnation must create a real AX observer"
        );
        let first_registration_error =
            register_application_notification(&first_observer, first_pid, &notification);
        assert_eq!(
            first_registration_error, AX_ERROR_SUCCESS,
            "first application incarnation must accept the positive-control notification"
        );

        first_seed.terminate();
        thread::sleep(Duration::from_millis(150));

        let second_seed = launch_seed(&seed_executable, &second_state_path);
        let second_state = wait_for_seed(&second_state_path).expect("wait for second M06 seed");
        assert_eq!(second_state["schema"], "localview-v43-m06-seed-state-v1");
        assert_eq!(second_state["window_title"], WINDOW_TITLE);
        let second_pid = second_state["pid"].as_i64().expect("second M06 pid") as i32;
        let second_launch_marker = second_state["launch_marker"]
            .as_u64()
            .expect("second M06 launch marker");

        let second_incarnation = AxApplicationIncarnation::new(
            APPLICATION_IDENTITY,
            second_pid,
            second_launch_marker,
        );
        assert_ne!(
            first_incarnation, second_incarnation,
            "kill/relaunch of the same executable must create a new application incarnation"
        );
        assert_ne!(
            first_launch_marker, second_launch_marker,
            "independent seed launch markers must distinguish application lifetimes"
        );

        let retired = match authority.validate_current(first_binding, &second_incarnation) {
            AxObserverApplicationDecision::ApplicationReincarnated(retired) => retired,
            other => panic!("relaunch must retire old observer authority, got {other:?}"),
        };
        assert_eq!(retired.previous_application_incarnation(), &first_incarnation);
        assert_eq!(retired.current_application_incarnation(), &second_incarnation);
        assert_eq!(
            retired.recreate_directive(),
            AxObserverRecreateDirective::CreateObserverForCurrentApplication
        );
        assert_eq!(
            retired.invalidated_observer_creation_revision(),
            first_observer_revision
        );

        let second_binding = authority
            .rebind_after_reincarnation(retired, second_incarnation.clone())
            .expect("same semantic application may create a fresh observer for the new incarnation");
        assert!(second_binding.observer_creation_revision() > first_observer_revision);

        let (second_observer, second_observer_create_error) = create_observer(second_pid);
        assert_eq!(
            second_observer_create_error, AX_ERROR_SUCCESS,
            "second application incarnation must create a new real AX observer"
        );
        let second_registration_error =
            register_application_notification(&second_observer, second_pid, &notification);
        assert_eq!(
            second_registration_error, AX_ERROR_SUCCESS,
            "fresh observer must register against the relaunched application incarnation"
        );

        let record = serde_json::json!({
            "schema": "localview-v43-m06-real-provider-record-v1",
            "case_id": "M06",
            "candidate_sha": candidate_sha,
            "seed_executable": seed_executable,
            "application_identity": APPLICATION_IDENTITY,
            "first_pid": first_pid,
            "first_launch_marker": first_launch_marker,
            "first_observer_create_ax_error": first_observer_create_error,
            "first_registration_ax_error": first_registration_error,
            "first_observer_creation_revision": first_observer_revision,
            "second_pid": second_pid,
            "second_launch_marker": second_launch_marker,
            "application_incarnation_changed": first_incarnation != second_incarnation,
            "old_observer_binding_consumed": true,
            "recreate_directive": "create_observer_for_current_application",
            "second_observer_create_ax_error": second_observer_create_error,
            "second_registration_ax_error": second_registration_error,
            "second_observer_creation_revision": second_binding.observer_creation_revision(),
            "fresh_observer_revision_is_newer": second_binding.observer_creation_revision() > first_observer_revision,
            "run_loop_delivery_claimed": false,
            "ground_truth_source": "real_appkit_kill_relaunch_plus_seed_launch_marker_plus_ax_observer_registration",
        });
        fs::write(
            artifact_dir.join("M06-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M06 real-provider record"),
        )
        .expect("write M06 real-provider record");

        drop(first_observer);
        drop(second_observer);
        drop(second_seed);
    }
}
