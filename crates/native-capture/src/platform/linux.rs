use tauri::webview::PlatformWebview;
use webkit2gtk::{gio, SnapshotOptions, SnapshotRegion, WebViewExt};

use crate::{
    build_frame, CaptureCompletion, CaptureRequest, NativeCaptureBackend, NativeCaptureError,
};

pub(crate) fn capture(
    webview: PlatformWebview,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    let view = webview.inner();
    view.snapshot(
        SnapshotRegion::Visible,
        SnapshotOptions::NONE,
        None::<&gio::Cancellable>,
        move |result| {
            let frame = result
                .map_err(|error| NativeCaptureError::Platform(error.to_string()))
                .and_then(|surface| {
                    let mut png = Vec::new();
                    surface
                        .write_to_png(&mut png)
                        .map_err(|error| NativeCaptureError::Platform(error.to_string()))?;
                    build_frame(png, NativeCaptureBackend::WebKitGtk, request)
                });
            completion(frame);
        },
    );
}
