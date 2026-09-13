#[cfg(target_os = "macos")]
mod macos_real_provider_m04 {
    use std::{
        ffi::{c_char, c_void, CStr},
        fs,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        ptr,
        thread,
        time::{Duration, Instant},
    };

    use localview_macos_ax_provider::{
        AxElementBindingProvider, AxElementIdentity, AxElementOperationDecision,
        AxElementReacquireDirective, AxPermissionProvider, AxPermissionState,
    };

    type AxUiElementRef = *const c_void;
    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;

    const AX_ERROR_SUCCESS: i32 = 0;
    const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const TARGET_TITLE: &str = "LocalView M04 Target";
    const WINDOW_IDENTITY: &str = "window-title:LocalView M04 Seed";
    const SEMANTIC_IDENTITY: &str = "ax-title:LocalView M04 Target";
    const MAX_AX_DEPTH: usize = 20;
    const MAX_AX_NODES: usize = 8_000;
    const AX_STALL_TIMEOUT_SECONDS: f32 = 0.25;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CfStringRef,
            value: *mut CfTypeRef,
        ) -> i32;
        fn AXUIElementPerformAction(element: AxUiElementRef, action: CfStringRef) -> i32;
        fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, timeout_in_seconds: f32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            c_string: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFGetTypeID(value: CfTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFArrayGetTypeID() -> usize;
        fn CFArrayGetCount(array: CfArrayRef) -> isize;
        fn CFArrayGetValueAtIndex(array: CfArrayRef, index: isize) -> *const c_void;
        fn CFStringGetCString(
            string: CfStringRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
        fn CFRetain(value: CfTypeRef) -> CfTypeRef;
        fn CFRelease(value: CfTypeRef);
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

    struct SeedProcess {
        child: Child,
        command_path: PathBuf,
    }

    impl Drop for SeedProcess {
        fn drop(&mut self) {
            let _ = fs::write(&self.command_path, "quit\n");
            thread::sleep(Duration::from_millis(100));
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    struct AxNames {
        children: OwnedCf,
        role: OwnedCf,
        title: OwnedCf,
        press: OwnedCf,
    }

    impl AxNames {
        fn new() -> Result<Self, String> {
            Ok(Self {
                children: cf_string_literal(b"AXChildren\0")?,
                role: cf_string_literal(b"AXRole\0")?,
                title: cf_string_literal(b"AXTitle\0")?,
                press: cf_string_literal(b"AXPress\0")?,
            })
        }
    }

    fn cf_string_literal(bytes: &'static [u8]) -> Result<OwnedCf, String> {
        let value = unsafe {
            CFStringCreateWithCString(
                ptr::null(),
                bytes.as_ptr().cast::<c_char>(),
                CF_STRING_ENCODING_UTF8,
            )
        };
        OwnedCf::new(value).ok_or_else(|| "CFStringCreateWithCString returned null".to_owned())
    }

    fn copy_attribute_result(
        element: AxUiElementRef,
        attribute: CfStringRef,
    ) -> (i32, Option<OwnedCf>) {
        let mut value: CfTypeRef = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
        let value = if error == AX_ERROR_SUCCESS {
            OwnedCf::new(value)
        } else {
            None
        };
        (error, value)
    }

    fn cf_string_value(value: CfTypeRef) -> Option<String> {
        if value.is_null() || unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() } {
            return None;
        }
        let mut buffer = [0 as c_char; 512];
        let copied = unsafe {
            CFStringGetCString(
                value.cast(),
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                CF_STRING_ENCODING_UTF8,
            )
        };
        if copied == 0 {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn string_attribute(element: AxUiElementRef, attribute: CfStringRef) -> Option<String> {
        let (error, value) = copy_attribute_result(element, attribute);
        if error != AX_ERROR_SUCCESS {
            return None;
        }
        value.and_then(|value| cf_string_value(value.raw()))
    }

    fn find_button(
        element: AxUiElementRef,
        names: &AxNames,
        depth: usize,
        visited: &mut usize,
    ) -> Option<OwnedCf> {
        if element.is_null() || depth > MAX_AX_DEPTH || *visited >= MAX_AX_NODES {
            return None;
        }
        *visited += 1;

        let role = string_attribute(element, names.role.raw().cast());
        let title = string_attribute(element, names.title.raw().cast());
        if role.as_deref() == Some("AXButton") && title.as_deref() == Some(TARGET_TITLE) {
            return OwnedCf::new(unsafe { CFRetain(element.cast()) });
        }

        let (error, children) = copy_attribute_result(element, names.children.raw().cast());
        if error != AX_ERROR_SUCCESS {
            return None;
        }
        let children = children?;
        if unsafe { CFGetTypeID(children.raw()) } != unsafe { CFArrayGetTypeID() } {
            return None;
        }

        let array: CfArrayRef = children.raw().cast();
        let count = unsafe { CFArrayGetCount(array) };
        for index in 0..count {
            let child = unsafe { CFArrayGetValueAtIndex(array, index) }.cast::<c_void>();
            if let Some(found) = find_button(child, names, depth + 1, visited) {
                return Some(found);
            }
        }
        None
    }

    fn resolve_target_button(pid: i32, names: &AxNames) -> Result<(OwnedCf, usize), String> {
        let app = OwnedCf::new(unsafe { AXUIElementCreateApplication(pid) }.cast())
            .ok_or_else(|| "AXUIElementCreateApplication returned null".to_owned())?;
        let mut visited = 0;
        let button = find_button(app.raw().cast(), names, 0, &mut visited)
            .ok_or_else(|| format!("M04 target AXButton was not found; visited={visited}"))?;
        Ok((button, visited))
    }

    fn read_state(path: &Path) -> Result<serde_json::Value, String> {
        let bytes = fs::read(path).map_err(|error| format!("read M04 seed state: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("parse M04 seed state: {error}"))
    }

    fn wait_for_state(
        path: &Path,
        expected_stalling: bool,
        min_stall_count: u64,
        min_press_count: u64,
    ) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        while Instant::now() < deadline {
            if let Ok(state) = read_state(path) {
                let stalling = state["stalling"].as_bool().unwrap_or(false);
                let stall_count = state["stall_count"].as_u64().unwrap_or(0);
                let press_count = state["press_count"].as_u64().unwrap_or(0);
                if stalling == expected_stalling
                    && stall_count >= min_stall_count
                    && press_count >= min_press_count
                {
                    return Ok(state);
                }
                last = Some(state);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(format!(
            "M04 seed state did not reach stalling={expected_stalling} stall_count>={min_stall_count} press_count>={min_press_count}; last={last:?}"
        ))
    }

    #[test]
    #[ignore = "requires real macOS AppKit + Accessibility provider behavior"]
    fn m04_unresponsive_target_returns_cannot_complete_and_requires_reacquire() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M04 requires LOCALVIEW_CANDIDATE_SHA");
        let seed_executable = std::env::var("LOCALVIEW_M04_SEED_EXECUTABLE")
            .expect("M04 requires exact-head seed executable");
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M04 requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M04 artifact directory");
        let state_path = artifact_dir.join("M04-SEED-STATE.json");
        let command_path = artifact_dir.join("M04-SEED-COMMAND.txt");
        let _ = fs::remove_file(&state_path);
        let _ = fs::remove_file(&command_path);

        let permission = AxPermissionProvider::new().current_permission_revision(false);
        assert_eq!(
            permission.state(),
            AxPermissionState::Trusted,
            "M04 requires a real trusted Accessibility topology"
        );

        let child = Command::new(&seed_executable)
            .env("LOCALVIEW_M04_STATE_PATH", &state_path)
            .env("LOCALVIEW_M04_COMMAND_PATH", &command_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch exact-head M04 AppKit seed");
        let mut seed = SeedProcess {
            child,
            command_path: command_path.clone(),
        };

        let ready = wait_for_state(&state_path, false, 0, 0).expect("wait for M04 seed ready");
        let pid = ready["pid"].as_i64().expect("seed state pid") as i32;
        assert_eq!(ready["target_title"], TARGET_TITLE);
        assert_eq!(ready["press_count"].as_u64(), Some(0));

        let names = AxNames::new().expect("build AX attribute names");
        let (button, initial_visited) =
            resolve_target_button(pid, &names).expect("resolve responsive M04 target button");
        let timeout_error = unsafe {
            AXUIElementSetMessagingTimeout(button.raw().cast(), AX_STALL_TIMEOUT_SECONDS)
        };
        assert_eq!(
            timeout_error, AX_ERROR_SUCCESS,
            "M04 requires a bounded AX messaging timeout on the real target"
        );

        let binding_provider = AxElementBindingProvider::new();
        let identity = AxElementIdentity::new(pid, WINDOW_IDENTITY, SEMANTIC_IDENTITY);
        let old_binding = binding_provider.bind_current(identity.clone());
        let old_binding_sequence = old_binding.binding_sequence();

        fs::write(&command_path, "stall\n").expect("request deterministic M04 main-thread stall");
        let stalled = wait_for_state(&state_path, true, 1, 0)
            .expect("independent seed ground truth must prove target is stalled");
        assert_eq!(stalled["stall_duration_ms"].as_u64(), Some(2_000));

        let started = Instant::now();
        let (cannot_complete_error, stalled_value) =
            copy_attribute_result(button.raw().cast(), names.title.raw().cast());
        let elapsed = started.elapsed();
        drop(stalled_value);
        assert_eq!(
            cannot_complete_error, AX_ERROR_CANNOT_COMPLETE,
            "a real AX request against the stalled AppKit target must return kAXErrorCannotComplete"
        );
        assert!(
            elapsed < Duration::from_millis(1_500),
            "M04 AX request must fail within the configured bounded timeout; elapsed={elapsed:?}"
        );

        let unresponsive = match binding_provider.observe_operation(old_binding, cannot_complete_error) {
            AxElementOperationDecision::UnresponsiveTarget(unresponsive) => unresponsive,
            other => panic!("real cannot-complete must consume authority as UnresponsiveTarget, got {other:?}"),
        };
        assert_eq!(
            unresponsive.invalidated_binding_sequence(),
            old_binding_sequence
        );
        assert_eq!(
            unresponsive.reacquire_directive(),
            AxElementReacquireDirective::ReacquireCurrentIdentity
        );

        // The timed-out operation is a read, so independent world-state proof
        // remains zero-effect. This isolates recovery semantics from duplicate
        // action ambiguity while still proving a real messaging timeout.
        let recovered = wait_for_state(&state_path, false, 1, 0)
            .expect("wait for target main thread to recover");
        assert_eq!(recovered["press_count"].as_u64(), Some(0));

        let (fresh_button, fresh_visited) = resolve_target_button(pid, &names)
            .expect("reacquire current target after unresponsive interval");
        let fresh_binding = binding_provider
            .rebind_after_unresponsive_reacquire(unresponsive, identity)
            .expect("same semantic identity must establish a new post-timeout binding");
        assert!(fresh_binding.binding_sequence() > old_binding_sequence);

        let reset_timeout_error = unsafe {
            AXUIElementSetMessagingTimeout(fresh_button.raw().cast(), 2.0)
        };
        assert_eq!(reset_timeout_error, AX_ERROR_SUCCESS);
        let press_error = unsafe {
            AXUIElementPerformAction(fresh_button.raw().cast(), names.press.raw().cast())
        };
        assert_eq!(
            press_error, AX_ERROR_SUCCESS,
            "reacquired responsive M04 target must accept one AXPress"
        );
        let pressed = wait_for_state(&state_path, false, 1, 1)
            .expect("verify exactly one post-reconcile world effect");
        assert_eq!(pressed["press_count"].as_u64(), Some(1));

        let record = serde_json::json!({
            "schema": "localview-v43-m04-real-provider-record-v1",
            "case_id": "M04",
            "candidate_sha": candidate_sha,
            "seed_executable": seed_executable,
            "seed_pid": pid,
            "semantic_identity": SEMANTIC_IDENTITY,
            "initial_ax_nodes_visited": initial_visited,
            "stall_ground_truth": true,
            "stall_count": stalled["stall_count"].as_u64(),
            "stall_duration_ms": stalled["stall_duration_ms"].as_u64(),
            "configured_ax_timeout_ms": 250,
            "cannot_complete_ax_error": cannot_complete_error,
            "cannot_complete_observed": cannot_complete_error == AX_ERROR_CANNOT_COMPLETE,
            "timeout_elapsed_ms": elapsed.as_millis(),
            "old_binding_sequence": old_binding_sequence,
            "old_binding_retried": false,
            "operation_effect_during_timeout": false,
            "reacquire_directive": "reacquire_current_identity",
            "fresh_ax_nodes_visited": fresh_visited,
            "fresh_binding_sequence": fresh_binding.binding_sequence(),
            "fresh_binding_is_newer": fresh_binding.binding_sequence() > old_binding_sequence,
            "fresh_ax_press_error": press_error,
            "fresh_world_press_count": pressed["press_count"].as_u64(),
            "ground_truth_source": "seed_state_file_test_only",
        });
        fs::write(
            artifact_dir.join("M04-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M04 record"),
        )
        .expect("write M04 record");

        fs::write(&command_path, "quit\n").ok();
        let _ = seed.child.wait();
    }
}
