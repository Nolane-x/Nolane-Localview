#[cfg(target_os = "macos")]
mod macos_real_provider_m05 {
    use std::{
        ffi::{c_char, c_void, CStr, CString},
        fs,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        ptr,
        thread,
        time::{Duration, Instant},
    };

    use localview_macos_ax_provider::{
        AxEventAssurance, AxNotificationRegistration, AxNotificationRegistrationOutcome,
        AxNotificationRequest, AxObserverReliabilityProfile, AxPermissionProvider,
        AxPermissionState,
    };

    type AxUiElementRef = *const c_void;
    type AxObserverRef = *const c_void;
    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;
    type CfArrayRef = *const c_void;

    const AX_ERROR_SUCCESS: i32 = 0;
    const AX_ERROR_NOTIFICATION_UNSUPPORTED: i32 = -25207;
    const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const POSITIVE_NOTIFICATION: &str = "AXFocusedUIElementChanged";
    const UNSUPPORTED_NOTIFICATION: &str = "AXSelectedRowsChanged";
    const WINDOW_TITLE: &str = "LocalView M05 Notification Seed";
    const MAX_AX_DEPTH: usize = 12;
    const MAX_AX_NODES: usize = 256;

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

    unsafe extern "C" fn observer_callback(
        _observer: AxObserverRef,
        _element: AxUiElementRef,
        _notification: CfStringRef,
        _refcon: *mut c_void,
    ) {
        // M05 proves registration reliability only. Event delivery/run-loop
        // liveness is intentionally reserved for M07.
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
        parent: OwnedCf,
        role: OwnedCf,
        value: OwnedCf,
        description: OwnedCf,
        identifier: OwnedCf,
        title: OwnedCf,
    }

    impl AxNames {
        fn new() -> Result<Self, String> {
            Ok(Self {
                children: cf_string("AXChildren")?,
                parent: cf_string("AXParent")?,
                role: cf_string("AXRole")?,
                value: cf_string("AXValue")?,
                description: cf_string("AXDescription")?,
                identifier: cf_string("AXIdentifier")?,
                title: cf_string("AXTitle")?,
            })
        }
    }

    struct StaticTextTarget {
        element: OwnedCf,
        value: Option<String>,
        description: Option<String>,
        identifier: Option<String>,
        title: Option<String>,
        parent_role: Option<String>,
        parent_value: Option<String>,
        parent_title: Option<String>,
    }

    fn cf_string(value: &str) -> Result<OwnedCf, String> {
        let value = CString::new(value)
            .map_err(|_| "CoreFoundation string contains interior NUL".to_owned())?;
        let string = unsafe {
            CFStringCreateWithCString(
                ptr::null(),
                value.as_ptr(),
                CF_STRING_ENCODING_UTF8,
            )
        };
        OwnedCf::new(string).ok_or_else(|| "CFStringCreateWithCString returned null".to_owned())
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

    fn collect_static_text_targets(
        element: AxUiElementRef,
        names: &AxNames,
        depth: usize,
        visited: &mut usize,
        targets: &mut Vec<StaticTextTarget>,
    ) {
        if element.is_null() || depth > MAX_AX_DEPTH || *visited >= MAX_AX_NODES {
            return;
        }
        *visited += 1;

        if string_attribute(element, names.role.raw().cast()).as_deref() == Some("AXStaticText") {
            let (parent_error, parent) = copy_attribute_result(element, names.parent.raw().cast());
            let (parent_role, parent_value, parent_title) = if parent_error == AX_ERROR_SUCCESS {
                if let Some(parent) = parent {
                    (
                        string_attribute(parent.raw().cast(), names.role.raw().cast()),
                        string_attribute(parent.raw().cast(), names.value.raw().cast()),
                        string_attribute(parent.raw().cast(), names.title.raw().cast()),
                    )
                } else {
                    (None, None, None)
                }
            } else {
                (None, None, None)
            };

            let retained = unsafe { CFRetain(element.cast()) };
            if let Some(retained) = OwnedCf::new(retained) {
                let target = StaticTextTarget {
                    value: string_attribute(retained.raw().cast(), names.value.raw().cast()),
                    description: string_attribute(retained.raw().cast(), names.description.raw().cast()),
                    identifier: string_attribute(retained.raw().cast(), names.identifier.raw().cast()),
                    title: string_attribute(retained.raw().cast(), names.title.raw().cast()),
                    parent_role,
                    parent_value,
                    parent_title,
                    element: retained,
                };
                targets.push(target);
            }
        }

        let (error, children) = copy_attribute_result(element, names.children.raw().cast());
        if error != AX_ERROR_SUCCESS {
            return;
        }
        let Some(children) = children else {
            return;
        };
        if unsafe { CFGetTypeID(children.raw()) } != unsafe { CFArrayGetTypeID() } {
            return;
        }
        let array: CfArrayRef = children.raw().cast();
        let count = unsafe { CFArrayGetCount(array) };
        for index in 0..count {
            let child = unsafe { CFArrayGetValueAtIndex(array, index) }.cast::<c_void>();
            collect_static_text_targets(child, names, depth + 1, visited, targets);
        }
    }

    fn read_seed_state(path: &Path) -> Result<serde_json::Value, String> {
        let bytes = fs::read(path).map_err(|error| format!("read M05 seed state: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("parse M05 seed state: {error}"))
    }

    fn wait_for_seed(path: &Path) -> Result<serde_json::Value, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = None;
        while Instant::now() < deadline {
            if let Ok(state) = read_seed_state(path) {
                if state["ready"].as_bool() == Some(true) {
                    return Ok(state);
                }
                last = Some(state);
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(format!("M05 seed did not become ready; last={last:?}"))
    }

    #[test]
    #[ignore = "requires real macOS AppKit + Accessibility observer behavior"]
    fn m05_unsupported_notification_forces_incomplete_event_assurance() {
        assert!(
            std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some(),
            "real macOS AX smoke must be explicitly enabled"
        );

        let candidate_sha = std::env::var("LOCALVIEW_CANDIDATE_SHA")
            .expect("M05 requires LOCALVIEW_CANDIDATE_SHA");
        let seed_executable = std::env::var("LOCALVIEW_M05_SEED_EXECUTABLE")
            .expect("M05 requires exact-head seed executable");
        let artifact_dir = PathBuf::from(
            std::env::var("LOCALVIEW_L7_ARTIFACT_DIR")
                .expect("M05 requires LOCALVIEW_L7_ARTIFACT_DIR"),
        );
        fs::create_dir_all(&artifact_dir).expect("create M05 artifact directory");
        let state_path = artifact_dir.join("M05-SEED-STATE.json");
        let _ = fs::remove_file(&state_path);

        let permission = AxPermissionProvider::new().current_permission_revision(false);
        assert_eq!(permission.state(), AxPermissionState::Trusted);

        let child = Command::new(&seed_executable)
            .env("LOCALVIEW_M05_STATE_PATH", &state_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("launch exact-head M05 AppKit seed");
        let _seed = SeedProcess(child);

        let state = wait_for_seed(&state_path).expect("wait for M05 seed ready");
        let pid = state["pid"].as_i64().expect("M05 seed pid") as i32;
        assert_eq!(state["window_title"], WINDOW_TITLE);

        let application = OwnedCf::new(unsafe { AXUIElementCreateApplication(pid) }.cast())
            .expect("AXUIElementCreateApplication returned null");
        let mut observer: AxObserverRef = ptr::null();
        let observer_create_error = unsafe { AXObserverCreate(pid, observer_callback, &mut observer) };
        assert_eq!(observer_create_error, AX_ERROR_SUCCESS);
        let observer = OwnedCf::new(observer.cast()).expect("AXObserverCreate returned null observer");

        let positive_notification = cf_string(POSITIVE_NOTIFICATION).unwrap();
        let supported_registration_error = unsafe {
            AXObserverAddNotification(
                observer.raw().cast(),
                application.raw().cast(),
                positive_notification.raw().cast(),
                ptr::null_mut(),
            )
        };
        assert_eq!(supported_registration_error, AX_ERROR_SUCCESS);

        let names = AxNames::new().expect("build M05 AX names");
        let mut visited = 0;
        let mut targets = Vec::new();
        collect_static_text_targets(
            application.raw().cast(),
            &names,
            0,
            &mut visited,
            &mut targets,
        );
        assert!(!targets.is_empty());

        let notification = cf_string(UNSUPPORTED_NOTIFICATION).unwrap();
        let mut unsupported_index = None;
        let mut results = Vec::new();
        for (index, target) in targets.iter().enumerate() {
            let ax_error = unsafe {
                AXObserverAddNotification(
                    observer.raw().cast(),
                    target.element.raw().cast(),
                    notification.raw().cast(),
                    ptr::null_mut(),
                )
            };
            eprintln!(
                "M05_STATIC_TEXT_IDENTITY index={} value={:?} description={:?} identifier={:?} title={:?} parent_role={:?} parent_value={:?} parent_title={:?} notification={} ax_error={}",
                index,
                target.value,
                target.description,
                target.identifier,
                target.title,
                target.parent_role,
                target.parent_value,
                target.parent_title,
                UNSUPPORTED_NOTIFICATION,
                ax_error,
            );
            results.push(ax_error);
            if ax_error == AX_ERROR_NOTIFICATION_UNSUPPORTED {
                unsupported_index = Some(index);
                break;
            }
        }

        let index = unsupported_index.unwrap_or_else(|| {
            panic!(
                "M05 diagnostic found no unsupported AXStaticText; visited={visited} target_count={} results={results:?}",
                targets.len()
            )
        });
        let target = &targets[index];
        let unsupported_registration_error = results[index];

        let mut direct_read_value: CfTypeRef = ptr::null();
        let direct_read_error = unsafe {
            AXUIElementCopyAttributeValue(
                target.element.raw().cast(),
                names.role.raw().cast(),
                &mut direct_read_value,
            )
        };
        assert_eq!(direct_read_error, AX_ERROR_SUCCESS);
        let direct_read_value = OwnedCf::new(direct_read_value).expect("M05 AXRole read returned null");
        assert_eq!(cf_string_value(direct_read_value.raw()).as_deref(), Some("AXStaticText"));

        let supported_registration = AxNotificationRegistration::from_ax_error(
            AxNotificationRequest::new(POSITIVE_NOTIFICATION, "application.focused_element"),
            supported_registration_error,
        );
        let unsupported_registration = AxNotificationRegistration::from_ax_error(
            AxNotificationRequest::new(UNSUPPORTED_NOTIFICATION, "diagnostic.static_text.selected_rows"),
            unsupported_registration_error,
        );
        assert_eq!(
            unsupported_registration.outcome(),
            AxNotificationRegistrationOutcome::Unsupported
        );
        let profile = AxObserverReliabilityProfile::from_registrations(vec![
            supported_registration,
            unsupported_registration,
        ]);
        assert_eq!(profile.assurance(), AxEventAssurance::Incomplete);
        assert!(profile.requires_snapshot_reconciliation());

        let record = serde_json::json!({
            "schema": "localview-v43-m05-real-provider-record-v1",
            "case_id": "M05",
            "candidate_sha": candidate_sha,
            "observer_create_ax_error": observer_create_error,
            "supported_registration_ax_error": supported_registration_error,
            "unsupported_target_index": index,
            "unsupported_target_role": "AXStaticText",
            "unsupported_target_value": target.value,
            "unsupported_target_description": target.description,
            "unsupported_target_identifier": target.identifier,
            "unsupported_target_title": target.title,
            "unsupported_parent_role": target.parent_role,
            "unsupported_parent_value": target.parent_value,
            "unsupported_parent_title": target.parent_title,
            "unsupported_notification": UNSUPPORTED_NOTIFICATION,
            "unsupported_registration_ax_error": unsupported_registration_error,
            "unsupported_registration_outcome": "unsupported",
            "event_assurance": "incomplete",
            "snapshot_reconciliation_required": profile.requires_snapshot_reconciliation(),
            "direct_read_attribute": "AXRole",
            "direct_read_value": "AXStaticText",
            "direct_read_ax_error": direct_read_error,
            "direct_read_after_unsupported_succeeded": true,
            "run_loop_delivery_claimed": false,
            "diagnostic_identity_probe_retained": true,
            "ground_truth_source": "real_same_pid_ax_registration_results_plus_seed_identity",
        });
        fs::write(
            artifact_dir.join("M05-REAL-PROVIDER-RECORD.json"),
            serde_json::to_vec_pretty(&record).expect("serialize M05 diagnostic record"),
        )
        .expect("write M05 diagnostic record");
    }
}
