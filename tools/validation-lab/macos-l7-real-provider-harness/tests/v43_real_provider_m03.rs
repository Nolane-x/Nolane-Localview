#[cfg(target_os = "macos")]
mod macos_real_provider_m03 {
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
    const AX_ERROR_INVALID_UI_ELEMENT: i32 = -25202;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const TARGET_TITLE: &str = "LocalView M03 Target";
    const WINDOW_IDENTITY: &str = "window-title:LocalView M03 Seed";
    const SEMANTIC_IDENTITY: &str = "ax-title:LocalView M03 Target";
    const MAX_AX_DEPTH: usize = 20;
    const MAX_AX_NODES: usize = 8_000;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CfStringRef,
            value: *mut CfTypeRef,
        ) -> i32;
        fn AXUIElementPerformAction(element: AxUiElementRef, action: CfStringRef) -> i32;
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
        let button = find_button(app.raw().cast(), names, 0, &mut visited).ok_or_else(|| {
            format!("M03 target AXButton was not found; visited={visited}")
        })?;
        Ok((button, visited))
    }

    fn read_state(path: &Path) -> Result<serde_json::Value, String> {
        let bytes = fs::read(path).map_err(|error| format!("read seed state: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("parse seed state: {error}"))
    }

    fn wait_for_state(path: &Path, generation: u64, press_count: u64) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        while Instant::now() < deadline {
            if let Ok(state) = read_state(path) {
                let observed_generation = state["element_generation"].as_u64().unwrap_or(0);
                let observed_presses = state["press_count"].as_u64().unwrap_or(0);
                if observed_generation >= generation && observed_presses >= press_count {
                    return Ok(state);
                }
                last = Some(state);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(format!(
            "seed state did not reach generation={generation} press_count={press_count}; last={last:?}"
        ))
    }

    #[test]
    #[ignore = "requires real macOS AppKit + Accessibility provider behavior"]
    fn m03_recreated_control_invalidates_old_ax_ref_and_requires_fresh_binding() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M03 requires LOCALVIEW_CANDIDATE_SHA");
        let seed_executable = std::env::var("LOCALVIEW_M03_SEED_EXECUTABLE")
            .expect("M03 requires exact-head seed executable");
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M03 requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M03 artifact directory");
        let state_path = artifact_dir.join("M03-SEED-STATE.json");
        let command_path = artifact_dir.join("M03-SEED-COMMAND.txt");
        let _ = fs::remove_file(&state_path);
        let _ = fs::remove_file(&command_path);

        let permission = AxPermissionProvider::new().current_permission_revision(false);
        assert_eq!(
            permission.state(),
            AxPermissionState::Trusted,
            "M03 requires a real trusted Accessibility topology"
        );

        let child = Command::new(&seed_executable)
            .env("LOCALVIEW_M03_STATE_PATH", &state_path)
            .env("LOCALVIEW_M03_COMMAND_PATH", &command_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch exact-head M03 AppKit seed");
        let mut seed = SeedProcess {
            child,
            command_path: command_path.clone(),
        };

        let generation1 = wait_for_state(&state_path, 1, 0).expect("wait for generation-1 seed state");
        let pid = generation1["pid"]
            .as_i64()
            .expect("seed state pid") as i32;
        assert_eq!(generation1["target_title"], TARGET_TITLE);

        let names = AxNames::new().expect("build AX attribute names");
        let (old_button, old_visited) =
            resolve_target_button(pid, &names).expect("resolve generation-1 AX button");
        let binding_provider = AxElementBindingProvider::new();
        let identity = AxElementIdentity::new(pid, WINDOW_IDENTITY, SEMANTIC_IDENTITY);
        let old_binding = binding_provider.bind_current(identity.clone());
        let old_binding_sequence = old_binding.binding_sequence();

        fs::write(&command_path, "recreate\n").expect("request seed control recreation");
        let generation2 = wait_for_state(&state_path, 2, 0).expect("wait for generation-2 seed state");
        assert_eq!(generation2["element_generation"].as_u64(), Some(2));

        // Give AppKit one additional run-loop turn so the removed generation-1
        // NSButton is fully detached/deallocated before probing its remote AX ref.
        thread::sleep(Duration::from_millis(250));
        let (old_ref_error, old_title_value) =
            copy_attribute_result(old_button.raw().cast(), names.title.raw().cast());
        drop(old_title_value);
        assert_eq!(
            old_ref_error, AX_ERROR_INVALID_UI_ELEMENT,
            "M03 requires the real generation-1 AXUIElementRef to become invalid after recreate"
        );

        let stale = match binding_provider.observe_operation(old_binding, old_ref_error) {
            AxElementOperationDecision::StaleTarget(stale) => stale,
            other => panic!("real invalid AX ref must map to stale target, got {other:?}"),
        };
        assert_eq!(stale.invalidated_binding_sequence(), old_binding_sequence);
        assert_eq!(
            stale.reacquire_directive(),
            AxElementReacquireDirective::ReacquireCurrentIdentity
        );

        let (fresh_button, fresh_visited) =
            resolve_target_button(pid, &names).expect("reacquire generation-2 AX button by current semantic identity");
        let fresh_binding = binding_provider
            .rebind_after_reacquire(stale, identity)
            .expect("same current semantic identity must establish a new binding");
        assert!(fresh_binding.binding_sequence() > old_binding_sequence);

        let press_error = unsafe {
            AXUIElementPerformAction(fresh_button.raw().cast(), names.press.raw().cast())
        };
        assert_eq!(press_error, AX_ERROR_SUCCESS, "fresh generation-2 AX button must accept AXPress");
        let pressed_state = wait_for_state(&state_path, 2, 1).expect("verify generation-2 button world effect");
        assert_eq!(pressed_state["press_count"].as_u64(), Some(1));

        let record = serde_json::json!({
            "schema": "localview-v43-m03-real-provider-record-v1",
            "case_id": "M03",
            "candidate_sha": candidate_sha,
            "seed_executable": seed_executable,
            "seed_pid": pid,
            "semantic_identity": SEMANTIC_IDENTITY,
            "generation_before": 1,
            "generation_after": 2,
            "old_ax_nodes_visited": old_visited,
            "fresh_ax_nodes_visited": fresh_visited,
            "old_binding_sequence": old_binding_sequence,
            "old_ref_ax_error": old_ref_error,
            "old_ref_invalid_ui_element": old_ref_error == AX_ERROR_INVALID_UI_ELEMENT,
            "stale_binding_retried": false,
            "reacquire_directive": "reacquire_current_identity",
            "fresh_binding_sequence": fresh_binding.binding_sequence(),
            "fresh_binding_is_newer": fresh_binding.binding_sequence() > old_binding_sequence,
            "fresh_ax_press_error": press_error,
            "fresh_world_press_count": pressed_state["press_count"].as_u64(),
            "ground_truth_source": "seed_state_file_test_only",
        });
        fs::write(
            artifact_dir.join("M03-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M03 record"),
        )
        .expect("write M03 record");

        fs::write(&command_path, "quit\n").ok();
        let _ = seed.child.wait();
    }
}
