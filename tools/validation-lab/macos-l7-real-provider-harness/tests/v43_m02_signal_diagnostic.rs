#[cfg(target_os = "macos")]
mod macos_m02_signal_diagnostic {
    use std::{ffi::c_void, process::Command, ptr};

    use localview_macos_ax_provider::{AxPermissionProvider, AxPermissionState};

    type AxUiElementRef = *const c_void;
    type CfArrayRef = *const c_void;
    type CfTypeRef = *const c_void;

    const K_AX_ERROR_SUCCESS: i32 = 0;
    const K_AX_ERROR_API_DISABLED: i32 = -25211;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> AxUiElementRef;
        fn AXUIElementCopyAttributeNames(
            element: AxUiElementRef,
            names: *mut CfArrayRef,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: CfTypeRef);
    }

    fn system_wide_attribute_names_error() -> i32 {
        let element = unsafe { AXUIElementCreateSystemWide() };
        assert!(!element.is_null(), "system-wide AX element must be created");

        let mut names: CfArrayRef = ptr::null();
        let error = unsafe { AXUIElementCopyAttributeNames(element, &mut names) };
        if !names.is_null() {
            unsafe { CFRelease(names.cast()) };
        }
        unsafe { CFRelease(element.cast()) };
        error
    }

    #[test]
    #[ignore = "diagnostic for real macOS 26 Accessibility revocation semantics"]
    fn fresh_process_incarnation_observes_reset_while_existing_incarnation_stays_stale() {
        assert!(std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some());

        let provider = AxPermissionProvider::new();
        let revision = provider.current_permission_revision(false);
        let message_error = system_wide_attribute_names_error();

        if std::env::var_os("LOCALVIEW_M02_FRESH_CHILD").is_some() {
            eprintln!(
                "M02_FRESH_CHILD trust={:?} ax_error={}",
                revision.state(),
                message_error,
            );
            assert_eq!(
                revision.state(),
                AxPermissionState::Untrusted,
                "fresh same-executable incarnation must observe the reset Accessibility authorization"
            );
            assert_eq!(
                message_error, K_AX_ERROR_API_DISABLED,
                "fresh same-executable incarnation must be denied AX messaging after reset"
            );
            return;
        }

        assert_eq!(
            revision.state(),
            AxPermissionState::Trusted,
            "diagnostic requires initially trusted hosted-runner topology"
        );
        assert_eq!(
            message_error, K_AX_ERROR_SUCCESS,
            "pre-reset AX messaging must succeed for this diagnostic"
        );

        let reset = Command::new("sudo")
            .args(["/usr/bin/tccutil", "reset", "Accessibility"])
            .status()
            .expect("invoke Accessibility TCC reset");
        assert!(reset.success(), "TCC reset must succeed");

        let stale_parent_revision = provider.current_permission_revision(false);
        let stale_parent_message_error = system_wide_attribute_names_error();
        assert_eq!(
            stale_parent_revision.state(),
            AxPermissionState::Trusted,
            "observed macOS 26 semantics keep the already-running incarnation trusted after reset"
        );
        assert_eq!(
            stale_parent_message_error, K_AX_ERROR_SUCCESS,
            "observed macOS 26 semantics keep AX messaging alive in the already-running incarnation"
        );

        let current_exe = std::env::current_exe().expect("resolve exact diagnostic test executable");
        let child = Command::new(current_exe)
            .env("LOCALVIEW_M02_FRESH_CHILD", "1")
            .arg("macos_m02_signal_diagnostic::fresh_process_incarnation_observes_reset_while_existing_incarnation_stays_stale")
            .arg("--ignored")
            .arg("--exact")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .status()
            .expect("spawn fresh same-executable M02 permission probe");

        assert!(
            child.success(),
            "fresh same-executable incarnation must observe the revoked Accessibility topology even while the old incarnation remains stale-trusted"
        );
    }
}
