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
    fn compare_trust_boolean_with_live_ax_messaging_after_tcc_reset() {
        assert!(std::env::var_os("LOCALVIEW_MACOS_AX_SMOKE").is_some());

        let provider = AxPermissionProvider::new();
        let before = provider.current_permission_revision(false);
        assert_eq!(
            before.state(),
            AxPermissionState::Trusted,
            "diagnostic requires initially trusted hosted-runner topology"
        );

        let before_message_error = system_wide_attribute_names_error();
        assert_eq!(
            before_message_error, K_AX_ERROR_SUCCESS,
            "pre-reset AX messaging must succeed for this diagnostic"
        );

        let reset = Command::new("sudo")
            .args(["/usr/bin/tccutil", "reset", "Accessibility"])
            .status()
            .expect("invoke Accessibility TCC reset");
        assert!(reset.success(), "TCC reset must succeed");

        let after = provider.current_permission_revision(false);
        let after_message_error = system_wide_attribute_names_error();

        eprintln!(
            "M02_SIGNAL_DIAGNOSTIC before_trust={:?} before_ax_error={} after_trust={:?} after_ax_error={}",
            before.state(),
            before_message_error,
            after.state(),
            after_message_error,
        );

        assert_eq!(
            after.state(),
            AxPermissionState::Trusted,
            "the observed macOS 26 failure mode is a stale Trusted boolean after reset"
        );
        assert_eq!(
            after_message_error, K_AX_ERROR_API_DISABLED,
            "hypothesis: live AX messaging must expose revoked TCC even when AXIsProcessTrusted stays stale"
        );
    }
}
