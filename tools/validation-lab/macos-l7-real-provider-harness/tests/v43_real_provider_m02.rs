#[cfg(target_os = "macos")]
mod macos_real_provider_m02 {
    use std::{
        ffi::{c_char, c_void, CStr},
        fs,
        path::PathBuf,
        process::Command,
        ptr,
        thread,
        time::Duration,
    };

    use localview_macos_ax_provider::{
        AxPermissionError, AxPermissionProvider, AxPermissionState, AxSemanticControlOutcome,
    };

    type AxUiElementRef = *const c_void;
    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;

    const AX_ERROR_SUCCESS: i32 = 0;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const MAX_AX_DEPTH: usize = 24;
    const MAX_AX_NODES: usize = 12_000;

    const ACCESSIBILITY_DEEP_LINK: &str =
        "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> AxUiElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AxUiElementRef,
            attribute: CfStringRef,
            value: *mut CfTypeRef,
        ) -> i32;
        fn AXUIElementCopyActionNames(element: AxUiElementRef, names: *mut CfArrayRef) -> i32;
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
        if bytes.last().copied() != Some(0) {
            return Err("CoreFoundation string literal must be NUL terminated".to_owned());
        }
        let value = unsafe {
            CFStringCreateWithCString(
                ptr::null(),
                bytes.as_ptr().cast::<c_char>(),
                CF_STRING_ENCODING_UTF8,
            )
        };
        OwnedCf::new(value).ok_or_else(|| "CFStringCreateWithCString returned null".to_owned())
    }

    fn copy_attribute(element: AxUiElementRef, attribute: CfStringRef) -> Option<OwnedCf> {
        let mut value: CfTypeRef = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
        if error == AX_ERROR_SUCCESS {
            OwnedCf::new(value)
        } else {
            None
        }
    }

    fn cf_string_value(value: CfTypeRef) -> Option<String> {
        if value.is_null()
            || unsafe { CFGetTypeID(value) } != unsafe { CFStringGetTypeID() }
        {
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

    fn find_bash_switch(
        element: AxUiElementRef,
        names: &AxNames,
        depth: usize,
        visited: &mut usize,
        seen_checkbox_titles: &mut Vec<String>,
    ) -> Option<OwnedCf> {
        if element.is_null() || depth > MAX_AX_DEPTH || *visited >= MAX_AX_NODES {
            return None;
        }
        *visited += 1;

        let role = copy_attribute(element, names.role.raw().cast())
            .and_then(|value| cf_string_value(value.raw()));
        let title = copy_attribute(element, names.title.raw().cast())
            .and_then(|value| cf_string_value(value.raw()));

        if role.as_deref() == Some("AXCheckBox") {
            if let Some(title) = title.as_ref() {
                if seen_checkbox_titles.len() < 64 {
                    seen_checkbox_titles.push(title.clone());
                }
                if title == "bash" {
                    let retained = unsafe { CFRetain(element.cast()) };
                    return OwnedCf::new(retained);
                }
            }
        }

        let children = copy_attribute(element, names.children.raw().cast())?;
        if unsafe { CFGetTypeID(children.raw()) } != unsafe { CFArrayGetTypeID() } {
            return None;
        }

        let array = children.raw().cast();
        let count = unsafe { CFArrayGetCount(array) };
        for index in 0..count {
            let child = unsafe { CFArrayGetValueAtIndex(array, index) }.cast::<c_void>();
            if let Some(found) = find_bash_switch(
                child,
                names,
                depth + 1,
                visited,
                seen_checkbox_titles,
            ) {
                return Some(found);
            }
        }
        None
    }

    fn copy_action_names(element: AxUiElementRef) -> Result<Vec<String>, String> {
        let mut array: CfArrayRef = ptr::null();
        let error = unsafe { AXUIElementCopyActionNames(element, &mut array) };
        if error != AX_ERROR_SUCCESS {
            return Err(format!("AXUIElementCopyActionNames failed with AXError {error}"));
        }
        let array = OwnedCf::new(array.cast())
            .ok_or_else(|| "AXUIElementCopyActionNames returned null".to_owned())?;
        if unsafe { CFGetTypeID(array.raw()) } != unsafe { CFArrayGetTypeID() } {
            return Err("AX action names result was not a CFArray".to_owned());
        }

        let array_ref = array.raw().cast();
        let count = unsafe { CFArrayGetCount(array_ref) };
        let mut names = Vec::with_capacity(count.max(0) as usize);
        for index in 0..count {
            let value = unsafe { CFArrayGetValueAtIndex(array_ref, index) };
            if let Some(name) = cf_string_value(value) {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn system_settings_pid() -> Result<i32, String> {
        let output = Command::new("/usr/bin/pgrep")
            .args(["-x", "System Settings"])
            .output()
            .map_err(|error| format!("launch pgrep for System Settings: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "System Settings process not found yet (pgrep status {})",
                output.status
            ));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .find_map(|line| line.trim().parse::<i32>().ok())
            .ok_or_else(|| format!("pgrep returned no parseable System Settings pid: {stdout:?}"))
    }

    fn revoke_bash_accessibility_via_native_ax() -> Result<(), String> {
        let names = AxNames::new()?;
        let mut last_seen_checkbox_titles = Vec::new();
        let mut last_visited = 0usize;

        for _ in 0..100 {
            if let Ok(pid) = system_settings_pid() {
                let application = unsafe { AXUIElementCreateApplication(pid) };
                if let Some(application) = OwnedCf::new(application.cast()) {
                    let mut visited = 0usize;
                    let mut seen_checkbox_titles = Vec::new();
                    if let Some(bash_switch) = find_bash_switch(
                        application.raw().cast(),
                        &names,
                        0,
                        &mut visited,
                        &mut seen_checkbox_titles,
                    ) {
                        let actions = copy_action_names(bash_switch.raw().cast())?;
                        if !actions.iter().any(|action| action == "AXPress") {
                            return Err(format!(
                                "bash Accessibility switch does not expose AXPress; actions={actions:?}"
                            ));
                        }

                        let press_error = unsafe {
                            AXUIElementPerformAction(
                                bash_switch.raw().cast(),
                                names.press.raw().cast(),
                            )
                        };
                        if press_error != AX_ERROR_SUCCESS {
                            return Err(format!(
                                "AXUIElementPerformAction(AXPress) failed with AXError {press_error}; actions={actions:?}"
                            ));
                        }

                        eprintln!(
                            "M02_NATIVE_AX_REVOKE pid={pid} visited={visited} action=AXPress result=success"
                        );
                        return Ok(());
                    }
                    last_seen_checkbox_titles = seen_checkbox_titles;
                    last_visited = visited;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }

        Err(format!(
            "bash Accessibility switch was not found through native AX traversal; last_visited={last_visited} checkbox_titles={last_seen_checkbox_titles:?}"
        ))
    }

    #[test]
    #[ignore = "requires the real macOS Accessibility permission regime"]
    fn m02_permission_revoked_mid_session_invalidates_prior_semantic_authority() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );

        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("real-provider M02 oracle requires LOCALVIEW_CANDIDATE_SHA");
        assert!(
            !candidate_sha.trim().is_empty(),
            "candidate SHA must not be empty"
        );

        let provider = AxPermissionProvider::new();
        let admission = provider.semantic_control_decision(false);
        let admitted_revision = admission.revision();

        assert_eq!(
            admitted_revision.state(),
            AxPermissionState::Trusted,
            "M02 requires a real initially trusted Accessibility topology; if the runner is already untrusted, do not fabricate a revocation pass"
        );
        let admitted_permit = admission
            .permit()
            .expect("trusted M02 precondition must mint semantic-control admission authority");
        assert_eq!(
            admitted_permit.permission_check_sequence(),
            admitted_revision.check_sequence(),
            "admitted permit must bind the exact initially trusted revision"
        );

        let open_settings = Command::new("/usr/bin/open")
            .arg(ACCESSIBILITY_DEEP_LINK)
            .status()
            .expect("open real Privacy & Security > Accessibility pane");
        assert!(
            open_settings.success(),
            "M02 requires the real Accessibility privacy pane to open"
        );

        revoke_bash_accessibility_via_native_ax()
            .expect("M02 requires a native AX mid-session Accessibility revoke of the responsible bash process");

        let dispatch = provider.semantic_dispatch_decision(admitted_permit);
        let dispatch_revision = dispatch.revision();

        assert_eq!(
            dispatch.admitted_permission_check_sequence(),
            admitted_permit.permission_check_sequence(),
            "dispatch fence must identify the exact authority revision being invalidated"
        );
        assert!(
            dispatch_revision.check_sequence() > admitted_revision.check_sequence(),
            "dispatch must take a newer provider-owned OS permission observation"
        );
        assert_eq!(
            dispatch_revision.state(),
            AxPermissionState::Untrusted,
            "the real System Settings Accessibility revoke must be observed before dispatch"
        );
        assert_eq!(
            dispatch.outcome(),
            AxSemanticControlOutcome::Denied(AxPermissionError::PermissionRevoked),
            "revoked permission must invalidate prior semantic-control authority with a typed denial"
        );
        assert_eq!(
            dispatch.permit(),
            None,
            "revoked dispatch must not mint replacement semantic-control authority"
        );

        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("real-provider M02 oracle requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M02 artifact directory");
        let record = serde_json::json!({
            "schema": "localview-v43-m02-real-provider-record-v1",
            "case_id": "M02",
            "candidate_sha": candidate_sha,
            "initial_permission_state": "trusted",
            "initial_permission_check_sequence": admitted_revision.check_sequence(),
            "initial_semantic_control_permit_minted": true,
            "system_settings_accessibility_pane_opened": true,
            "responsible_process_switch": "bash",
            "system_settings_bash_switch_toggled_off_mid_session": true,
            "native_ax_press_action_verified": true,
            "dispatch_permission_state": "untrusted",
            "dispatch_permission_check_sequence": dispatch_revision.check_sequence(),
            "admitted_permission_check_sequence": dispatch.admitted_permission_check_sequence(),
            "dispatch_semantic_control_permit_minted": false,
            "typed_permission_revocation": true,
            "denial": "permission_revoked",
            "authority_source": "provider_owned_dispatch_recheck_after_user_equivalent_system_settings_revoke",
        });
        fs::write(
            artifact_dir.join("M02-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M02 record"),
        )
        .expect("write M02 real-provider record");
    }
}
