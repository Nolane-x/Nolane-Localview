use tauri::webview::PlatformWebview;

use crate::{CaptureCompletion, CaptureRequest, NativeCaptureError};

pub(crate) fn capture(
    _webview: PlatformWebview,
    _request: CaptureRequest,
    completion: CaptureCompletion,
) {
    completion(Err(NativeCaptureError::NotReady));
}
