#[cfg(target_os = "macos")]
mod macos_real_provider_m07 {
    use std::{
        ffi::{c_char, c_void, CString},
        fs,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        ptr,
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use localview_macos_ax_provider::{
        AxApplicationIncarnation, AxObserverApplicationBindingProvider,
        AxObserverContinuityBreakReason, AxObserverContinuityDecision,
        AxObserverContinuityProvider, AxPermissionProvider, AxPermissionState, AxRunLoopLiveness,
    };

    type AxUiElementRef = *const c_void;
    type AxObserverRef = *const c_void;
    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;
    type CfRunLoopRef = *const c_void;
    type CfRunLoopSourceRef = *const c_void;

    const AX_ERROR_SUCCESS: i32 = 0;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const APPLICATION_IDENTITY: &str = "com.nolane.localview.m07-seed";
    const BASE_TITLE: &str = "LocalView M07 RunLoop Seed";
    const NOTIFICATION: &str = "AXTitleChanged";

    static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CfStringRef,
            value: *mut CfTypeRef,
        ) -> i32;
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
        fn AXObserverGetRunLoopSource(observer: AxObserverRef) -> CfRunLoopSourceRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopDefaultMode: CfStringRef;
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            c_string: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFArrayGetCount(array: CfArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CfArrayRef, index: isize) -> *const c_void;
        fn CFRetain(value: CfTypeRef) -> CfTypeRef;
        fn CFRelease(value: CfTypeRef);
        fn CFRunLoopGetCurrent() -> CfRunLoopRef;
        fn CFRunLoopAddSource(
            run_loop: CfRunLoopRef,
            source: CfRunLoopSourceRef,
            mode: CfStringRef,
        );
        fn CFRunLoopRemoveSource(
            run_loop: CfRunLoopRef,
            source: CfRunLoopSourceRef,
            mode: CfStringRef,
        );
        fn CFRunLoopContainsSource(
            run_loop: CfRunLoopRef,
            source: CfRunLoopSourceRef,
            mode: CfStringRef,
        ) -> u8;
        fn CFRunLoopRunInMode(
            mode: CfStringRef,
            seconds: f64,
            return_after_source_handled: u8,
        ) -> i32;
    }

    unsafe extern "C" fn observer_callback(
        _observer: AxObserverRef,
        _element: AxUiElementRef,
        _notification: CfStringRef,
        _refcon: *mut c_void,
    ) {
        CALLBACK_COUNT.fetch_add(1, Ordering::AcqRel);
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

    fn copy_attribute(
        element: AxUiElementRef,
        attribute: CfStringRef,
    ) -> Result<OwnedCf, String> {
        let mut value: CfTypeRef = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
        if error != AX_ERROR_SUCCESS {
            return Err(format!("AXUIElementCopyAttributeValue failed: {error}"));
        }
        OwnedCf::new(value).ok_or_else(|| "AX attribute returned null".to_owned())
    }

    fn resolve_first_window(pid: i32) -> Result<OwnedCf, String> {
        let application = OwnedCf::new(unsafe { AXUIElementCreateApplication(pid) }.cast())
            .ok_or_else(|| "AXUIElementCreateApplication returned null".to_owned())?;
        let windows_attribute = cf_string("AXWindows")?;
        let windows = copy_attribute(application.raw().cast(), windows_attribute.raw().cast())?;
        let array: CfArrayRef = windows.raw().cast();
        let count = unsafe { CFArrayGetCount(array) };
        if count < 1 {
            return Err("M07 seed exposed no AX windows".to_owned());
        }
        let window = unsafe { CFArrayGetValueAtIndex(array, 0) }.cast::<c_void>();
        OwnedCf::new(unsafe { CFRetain(window) })
            .ok_or_else(|| "M07 first AX window was null".to_owned())
    }

    fn read_state(path: &Path) -> Result<serde_json::Value, String> {
        let bytes = fs::read(path).map_err(|error| format!("read M07 seed state: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("parse M07 seed state: {error}"))
    }

    fn wait_for_generation(path: &Path, generation: u64) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        while Instant::now() < deadline {
            if let Ok(state) = read_state(path) {
                let ready = state["ready"].as_bool() == Some(true);
                let observed = state["title_generation"].as_u64().unwrap_or(0);
                if ready && observed >= generation {
                    return Ok(state);
                }
                last = Some(state);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(format!(
            "M07 seed did not reach title_generation={generation}; last={last:?}"
        ))
    }

    fn create_observer(pid: i32) -> Result<(OwnedCf, i32), String> {
        let mut observer: AxObserverRef = ptr::null();
        let error = unsafe { AXObserverCreate(pid, observer_callback, &mut observer) };
        let observer = OwnedCf::new(observer.cast())
            .ok_or_else(|| format!("AXObserverCreate returned null; error={error}"))?;
        Ok((observer, error))
    }

    fn register_title_notification(observer: &OwnedCf, window: &OwnedCf) -> Result<i32, String> {
        let notification = cf_string(NOTIFICATION)?;
        Ok(unsafe {
            AXObserverAddNotification(
                observer.raw().cast(),
                window.raw().cast(),
                notification.raw().cast(),
                ptr::null_mut(),
            )
        })
    }

    fn run_loop_mode() -> CfStringRef {
        unsafe { kCFRunLoopDefaultMode }
    }

    fn add_observer_source(observer: &OwnedCf) -> Result<(CfRunLoopRef, CfRunLoopSourceRef), String> {
        let run_loop = unsafe { CFRunLoopGetCurrent() };
        if run_loop.is_null() {
            return Err("CFRunLoopGetCurrent returned null".to_owned());
        }
        let source = unsafe { AXObserverGetRunLoopSource(observer.raw().cast()) };
        if source.is_null() {
            return Err("AXObserverGetRunLoopSource returned null".to_owned());
        }
        unsafe { CFRunLoopAddSource(run_loop, source, run_loop_mode()) };
        if unsafe { CFRunLoopContainsSource(run_loop, source, run_loop_mode()) } == 0 {
            return Err("observer source was not attached to the current run loop".to_owned());
        }
        Ok((run_loop, source))
    }

    fn remove_observer_source(run_loop: CfRunLoopRef, source: CfRunLoopSourceRef) -> bool {
        unsafe { CFRunLoopRemoveSource(run_loop, source, run_loop_mode()) };
        (unsafe { CFRunLoopContainsSource(run_loop, source, run_loop_mode()) }) == 0
    }

    fn pump_until_callbacks(minimum: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if CALLBACK_COUNT.load(Ordering::Acquire) >= minimum {
                return true;
            }
            unsafe {
                CFRunLoopRunInMode(run_loop_mode(), 0.05, 0);
            }
            thread::sleep(Duration::from_millis(5));
        }
        CALLBACK_COUNT.load(Ordering::Acquire) >= minimum
    }

    fn pump_for(duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            unsafe {
                CFRunLoopRunInMode(run_loop_mode(), 0.03, 0);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    #[ignore = "requires real macOS AppKit + Accessibility observer/run-loop behavior"]
    fn m07_run_loop_interruption_breaks_and_recreation_restores_callback_continuity() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );
        CALLBACK_COUNT.store(0, Ordering::Release);

        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M07 requires LOCALVIEW_CANDIDATE_SHA");
        let seed_executable = std::env::var("LOCALVIEW_M07_SEED_EXECUTABLE")
            .expect("M07 requires exact-head seed executable");
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M07 requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M07 artifact directory");
        let state_path = artifact_dir.join("M07-SEED-STATE.json");
        let command_path = artifact_dir.join("M07-SEED-COMMAND.txt");
        let _ = fs::remove_file(&state_path);
        let _ = fs::remove_file(&command_path);

        let permission = AxPermissionProvider::new().current_permission_revision(false);
        assert_eq!(
            permission.state(),
            AxPermissionState::Trusted,
            "M07 requires a real trusted Accessibility topology"
        );

        let child = Command::new(&seed_executable)
            .env("LOCALVIEW_M07_STATE_PATH", &state_path)
            .env("LOCALVIEW_M07_COMMAND_PATH", &command_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch exact-head M07 AppKit seed");
        let _seed = SeedProcess(child);

        let initial = wait_for_generation(&state_path, 0).expect("wait for M07 seed ready");
        assert_eq!(initial["schema"], "localview-v43-m07-seed-state-v1");
        let pid = initial["pid"].as_i64().expect("M07 pid") as i32;
        let launch_marker = initial["launch_marker"]
            .as_u64()
            .expect("M07 launch marker");
        assert_eq!(initial["window_title"], format!("{BASE_TITLE} 0"));

        let window = resolve_first_window(pid).expect("resolve M07 AX window");
        let application_incarnation =
            AxApplicationIncarnation::new(APPLICATION_IDENTITY, pid, launch_marker);
        let observer_authority = AxObserverApplicationBindingProvider::new();
        let continuity_authority = AxObserverContinuityProvider::new();

        let first_binding = observer_authority.bind_current(application_incarnation.clone());
        let first_observer_revision = first_binding.observer_creation_revision();
        let (first_observer, first_create_error) =
            create_observer(pid).expect("create M07 observer 1");
        assert_eq!(first_create_error, AX_ERROR_SUCCESS);
        let first_registration_error = register_title_notification(&first_observer, &window)
            .expect("register M07 title notification 1");
        assert_eq!(
            first_registration_error, AX_ERROR_SUCCESS,
            "M07 positive-control AXTitleChanged notification must register"
        );
        let (run_loop, first_os_source) =
            add_observer_source(&first_observer).expect("attach first M07 observer source");
        let first_source_incarnation = continuity_authority.new_run_loop_source_incarnation();
        let live = continuity_authority
            .activate(
                first_binding,
                first_source_incarnation,
                AxRunLoopLiveness::Serviced,
            )
            .expect("attached and serviced first source must establish continuity");
        let first_continuity_revision = live.continuity_revision();

        fs::write(&command_path, "title 1\n").expect("request first title mutation");
        let first_changed =
            wait_for_generation(&state_path, 1).expect("wait for first title mutation");
        assert_eq!(first_changed["window_title"], format!("{BASE_TITLE} 1"));
        assert!(
            pump_until_callbacks(1, Duration::from_secs(5)),
            "serviced AX observer source must deliver the first real title callback"
        );
        pump_for(Duration::from_millis(150));
        let callbacks_before_interruption = CALLBACK_COUNT.load(Ordering::Acquire);
        assert!(callbacks_before_interruption >= 1);

        assert!(
            remove_observer_source(run_loop, first_os_source),
            "first AX observer run-loop source must be removed before interruption mutation"
        );
        let broken = match continuity_authority.validate(
            live,
            first_source_incarnation,
            AxRunLoopLiveness::Interrupted,
        ) {
            AxObserverContinuityDecision::Broken(broken) => broken,
            other => panic!("run-loop interruption must break M07 continuity, got {other:?}"),
        };
        assert_eq!(
            broken.reason(),
            AxObserverContinuityBreakReason::RunLoopInterrupted
        );
        assert!(broken.requires_snapshot_reconciliation());
        assert!(broken.requires_observer_recreation());

        fs::write(&command_path, "title 2\n").expect("request interruption title mutation");
        let interrupted =
            wait_for_generation(&state_path, 2).expect("wait for interruption title mutation");
        assert_eq!(interrupted["window_title"], format!("{BASE_TITLE} 2"));
        pump_for(Duration::from_millis(650));
        let callbacks_after_interruption = CALLBACK_COUNT.load(Ordering::Acquire);
        assert_eq!(
            callbacks_after_interruption, callbacks_before_interruption,
            "removed run-loop source must not deliver the title mutation during interruption"
        );

        drop(first_observer);

        let fresh_binding = observer_authority.bind_current(application_incarnation.clone());
        let second_observer_revision = fresh_binding.observer_creation_revision();
        assert!(second_observer_revision > first_observer_revision);
        let (second_observer, second_create_error) =
            create_observer(pid).expect("create M07 observer 2");
        assert_eq!(second_create_error, AX_ERROR_SUCCESS);
        let second_registration_error = register_title_notification(&second_observer, &window)
            .expect("register M07 title notification 2");
        assert_eq!(second_registration_error, AX_ERROR_SUCCESS);
        let (second_run_loop, second_os_source) =
            add_observer_source(&second_observer).expect("attach second M07 observer source");
        assert_eq!(second_run_loop, run_loop);
        let second_source_incarnation = continuity_authority.new_run_loop_source_incarnation();
        assert_ne!(second_source_incarnation, first_source_incarnation);
        let restored = continuity_authority
            .rebind_after_break(
                broken,
                fresh_binding,
                second_source_incarnation,
                AxRunLoopLiveness::Serviced,
            )
            .expect("fresh observer and source must restore M07 continuity");
        assert!(restored.continuity_revision() > first_continuity_revision);

        fs::write(&command_path, "title 3\n").expect("request restored title mutation");
        let restored_state =
            wait_for_generation(&state_path, 3).expect("wait for restored title mutation");
        assert_eq!(restored_state["window_title"], format!("{BASE_TITLE} 3"));
        assert!(
            pump_until_callbacks(callbacks_after_interruption + 1, Duration::from_secs(5)),
            "recreated serviced AX observer source must restore real callback delivery"
        );
        let callbacks_after_recovery = CALLBACK_COUNT.load(Ordering::Acquire);
        assert!(callbacks_after_recovery > callbacks_after_interruption);

        let second_source_removed = remove_observer_source(second_run_loop, second_os_source);
        assert!(second_source_removed);

        let record = serde_json::json!({
            "schema": "localview-v43-m07-real-provider-record-v1",
            "case_id": "M07",
            "candidate_sha": candidate_sha,
            "seed_executable": seed_executable,
            "application_identity": APPLICATION_IDENTITY,
            "pid": pid,
            "launch_marker": launch_marker,
            "notification": NOTIFICATION,
            "first_observer_create_ax_error": first_create_error,
            "first_registration_ax_error": first_registration_error,
            "first_observer_creation_revision": first_observer_revision,
            "first_run_loop_source_incarnation": first_source_incarnation.sequence(),
            "first_continuity_revision": first_continuity_revision,
            "initial_callback_delivered": callbacks_before_interruption >= 1,
            "callback_count_before_interruption": callbacks_before_interruption,
            "run_loop_source_removed_before_gap": true,
            "interrupted_title_generation": interrupted["title_generation"].as_u64(),
            "callback_count_after_interruption": callbacks_after_interruption,
            "callback_delivery_absent_during_interruption": callbacks_after_interruption == callbacks_before_interruption,
            "continuity_broken": true,
            "break_reason": "run_loop_interrupted",
            "snapshot_reconciliation_required": true,
            "observer_recreation_required": true,
            "second_observer_create_ax_error": second_create_error,
            "second_registration_ax_error": second_registration_error,
            "second_observer_creation_revision": second_observer_revision,
            "fresh_observer_revision_is_newer": second_observer_revision > first_observer_revision,
            "second_run_loop_source_incarnation": second_source_incarnation.sequence(),
            "fresh_run_loop_source_incarnation": second_source_incarnation != first_source_incarnation,
            "restored_continuity_revision": restored.continuity_revision(),
            "restored_continuity_revision_is_newer": restored.continuity_revision() > first_continuity_revision,
            "restored_callback_delivered": callbacks_after_recovery > callbacks_after_interruption,
            "callback_count_after_recovery": callbacks_after_recovery,
            "event_continuity_restored": callbacks_after_recovery > callbacks_after_interruption,
            "second_run_loop_source_removed_on_cleanup": second_source_removed,
            "run_loop_delivery_proven": true,
            "ground_truth_source": "real_appkit_title_mutations_plus_real_axobserver_cfrunloop_source_removal_and_recreation",
        });
        fs::write(
            artifact_dir.join("M07-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M07 real-provider record"),
        )
        .expect("write M07 real-provider record");

        drop(second_observer);
    }
}
