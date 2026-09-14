#[cfg(target_os = "macos")]
mod macos_real_provider_m09 {
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
        AxPermissionProvider, AxPermissionState, AxSensitiveTextMetadata, AxSensitiveTextPolicy,
        AxSensitiveTextProtection, AxSensitiveTextTaint,
    };

    type AxUiElementRef = *const c_void;
    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;

    const AX_ERROR_SUCCESS: i32 = 0;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const MAX_AX_DEPTH: usize = 20;
    const MAX_AX_NODES: usize = 8_000;
    const SECRET_CANARY: &str = "LOCALVIEW_M09_SECRET_CANARY_DO_NOT_EXPORT";

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CfStringRef,
            value: *mut CfTypeRef,
        ) -> i32;
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

    struct SeedProcess(Child);

    impl Drop for SeedProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct AxNames {
        children: OwnedCf,
        role: OwnedCf,
        subrole: OwnedCf,
    }

    impl AxNames {
        fn new() -> Result<Self, String> {
            Ok(Self {
                children: cf_string_literal(b"AXChildren\0")?,
                role: cf_string_literal(b"AXRole\0")?,
                subrole: cf_string_literal(b"AXSubrole\0")?,
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

    fn find_secure_text_field(
        element: AxUiElementRef,
        names: &AxNames,
        depth: usize,
        visited: &mut usize,
    ) -> Option<(OwnedCf, String, String)> {
        if element.is_null() || depth > MAX_AX_DEPTH || *visited >= MAX_AX_NODES {
            return None;
        }
        *visited += 1;

        let role = string_attribute(element, names.role.raw().cast());
        let subrole = string_attribute(element, names.subrole.raw().cast());
        if role.as_deref() == Some("AXTextField")
            && subrole.as_deref() == Some("AXSecureTextField")
        {
            return Some((
                OwnedCf::new(unsafe { CFRetain(element.cast()) })?,
                role?,
                subrole?,
            ));
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
            if let Some(found) = find_secure_text_field(child, names, depth + 1, visited) {
                return Some(found);
            }
        }
        None
    }

    fn read_state(path: &Path) -> Result<serde_json::Value, String> {
        let bytes = fs::read(path).map_err(|error| format!("read M09 seed state: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("parse M09 seed state: {error}"))
    }

    fn wait_for_state(path: &Path) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        while Instant::now() < deadline {
            if let Ok(state) = read_state(path) {
                if state["pid"].as_i64().unwrap_or(0) > 0 {
                    return Ok(state);
                }
                last = Some(state);
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(format!("M09 seed state did not become ready; last={last:?}"))
    }

    #[test]
    #[ignore = "requires real macOS AppKit + Accessibility provider behavior"]
    fn m09_secure_text_is_classified_before_value_read_and_never_exported() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );
        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M09 requires LOCALVIEW_CANDIDATE_SHA");
        let seed_executable = std::env::var("LOCALVIEW_M09_SEED_EXECUTABLE")
            .expect("M09 requires exact-head seed executable");
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M09 requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M09 artifact directory");
        let state_path = artifact_dir.join("M09-SEED-STATE.json");
        let _ = fs::remove_file(&state_path);

        let permission = AxPermissionProvider::new().current_permission_revision(false);
        assert_eq!(
            permission.state(),
            AxPermissionState::Trusted,
            "M09 requires a real trusted Accessibility topology"
        );

        let child = Command::new(&seed_executable)
            .env("LOCALVIEW_M09_STATE_PATH", &state_path)
            .env("LOCALVIEW_M09_SECRET", SECRET_CANARY)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch exact-head M09 AppKit seed");
        let _seed = SeedProcess(child);

        let initial_state = wait_for_state(&state_path).expect("wait for M09 seed state");
        let pid = initial_state["pid"].as_i64().expect("M09 seed pid") as i32;
        assert_eq!(
            initial_state["value_read_count"].as_u64(),
            Some(0),
            "seed must begin with zero accessibility value reads"
        );
        assert!(
            !initial_state.to_string().contains(SECRET_CANARY),
            "test-only ground truth must never echo the protected plaintext"
        );

        let names = AxNames::new().expect("build M09 AX attribute names");
        let app = OwnedCf::new(unsafe { AXUIElementCreateApplication(pid) }.cast())
            .expect("AXUIElementCreateApplication returned null for M09 seed");
        let mut visited = 0;
        let (_field, role, subrole) = find_secure_text_field(
            app.raw().cast(),
            &names,
            0,
            &mut visited,
        )
        .unwrap_or_else(|| panic!("M09 AXSecureTextField not found; visited={visited}"));

        let decision = AxSensitiveTextPolicy::classify(AxSensitiveTextMetadata::new(
            &role,
            Some(&subrole),
        ));
        assert_eq!(
            decision.protection(),
            AxSensitiveTextProtection::ProtectedSecureText
        );
        assert_eq!(
            decision.semantic_text_protection(),
            Some("protected_secure_text")
        );
        assert!(!decision.value_read_permitted());
        assert_eq!(
            decision.taints(),
            &[AxSensitiveTextTaint::Secret, AxSensitiveTextTaint::Credential]
        );

        thread::sleep(Duration::from_millis(150));
        let after_state = read_state(&state_path).expect("read post-observation M09 seed state");
        let value_read_count = after_state["value_read_count"]
            .as_u64()
            .expect("M09 value_read_count");
        assert_eq!(
            value_read_count, 0,
            "M09 metadata-first observation must never invoke accessibilityValue for protected text"
        );
        assert!(
            !after_state.to_string().contains(SECRET_CANARY),
            "M09 ground-truth state must remain secret-safe"
        );

        let semantic_export = serde_json::json!({
            "role": role,
            "subrole": subrole,
            "semantic_text_protection": decision.semantic_text_protection(),
            "taints": ["secret", "credential"],
        });
        let semantic_export_text = serde_json::to_string(&semantic_export)
            .expect("serialize M09 semantic export");
        assert!(
            !semantic_export_text.contains(SECRET_CANARY),
            "M09 semantic export must contain only protection facts, never plaintext"
        );

        let record = serde_json::json!({
            "schema": "localview-v43-m09-real-provider-record-v1",
            "case_id": "M09",
            "candidate_sha": candidate_sha,
            "ax_role": semantic_export["role"],
            "ax_subrole": semantic_export["subrole"],
            "semantic_text_protection": semantic_export["semantic_text_protection"],
            "taints": ["secret", "credential"],
            "value_read_permitted": false,
            "provider_value_read_count": value_read_count,
            "semantic_export_contains_plaintext": false,
            "evidence_artifact_contains_plaintext": false,
            "ax_nodes_visited": visited,
            "ground_truth_source": "seed_state_file_test_only",
        });
        let record_text = serde_json::to_string_pretty(&record)
            .expect("serialize M09 real-provider record");
        assert!(
            !record_text.contains(SECRET_CANARY),
            "M09 evidence artifact must never retain protected plaintext"
        );
        fs::write(
            artifact_dir.join("M09-REAL-PROVIDER-RECORD.json"),
            record_text,
        )
        .expect("write M09 real-provider record");
    }
}
