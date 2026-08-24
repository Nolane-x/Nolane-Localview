// Platform-specific native pixel adapters are intentionally private to this crate.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

use tauri::webview::PlatformWebview;

use crate::{CaptureCompletion, CaptureRequest};

pub(crate) fn capture(
    webview: PlatformWebview,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    #[cfg(windows)]
    {
        windows::capture(webview, request, completion);
    }

    #[cfg(target_os = "macos")]
    {
        macos::capture(webview, request, completion);
    }

    #[cfg(target_os = "linux")]
    {
        linux::capture(webview, request, completion);
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = webview;
        let _ = request;
        completion(Err(crate::NativeCaptureError::UnsupportedPlatform));
    }
}

#[cfg(all(target_os = "macos", feature = "gui-smoke"))]
pub(crate) fn capture_wk_webview_for_gui_smoke(
    view: &objc2_web_kit::WKWebView,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    macos::capture_view(view, request, completion);
}
