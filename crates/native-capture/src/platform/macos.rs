use std::sync::Mutex;

use block2::RcBlock;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};
use tauri::webview::PlatformWebview;

use crate::{
    build_frame, validate_frame_size, CaptureCompletion, CaptureRequest, NativeCaptureBackend,
    NativeCaptureError,
};

pub(crate) fn capture(
    webview: PlatformWebview,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    let raw = webview.inner();
    if raw.is_null() {
        completion(Err(NativeCaptureError::NotReady));
        return;
    }

    // SAFETY: Tauri exposes the native pointer of the managed WKWebView. The
    // pointer is checked non-null above and this function is invoked from
    // `with_webview`, which guarantees the platform webview is alive for this
    // native call. The shared helper only starts WebKit's async snapshot work;
    // WebKit copies/retains the completion block for that work.
    let native = unsafe { &*raw.cast::<WKWebView>() };
    capture_view(native, request, completion);
}

pub(crate) fn capture_view(
    view: &WKWebView,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    let completion = Mutex::new(Some(completion));
    let request = Mutex::new(Some(request));
    let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let finish = |result: Result<crate::CapturedFrame, NativeCaptureError>| {
            if let Ok(mut guard) = completion.lock() {
                if let Some(completion) = guard.take() {
                    completion(result);
                }
            }
        };

        if !error.is_null() {
            finish(Err(NativeCaptureError::Platform(
                "WKWebView snapshot failed".into(),
            )));
            return;
        }
        let Some(image) = (unsafe { image.as_ref() }) else {
            finish(Err(NativeCaptureError::InvalidImage));
            return;
        };

        let properties = NSDictionary::new();
        let representations = image.representations();
        // SAFETY: `representations` comes directly from the immutable snapshot
        // NSImage and the properties dictionary is empty but correctly typed for
        // AppKit's PNG encoder.
        let Some(data) = (unsafe {
            NSBitmapImageRep::representationOfImageRepsInArray_usingType_properties(
                &representations,
                NSBitmapImageFileType::PNG,
                &properties,
            )
        }) else {
            finish(Err(NativeCaptureError::InvalidImage));
            return;
        };

        if let Err(error) = validate_frame_size(data.len()) {
            finish(Err(error));
            return;
        }
        let png = data.to_vec();
        let request = request.lock().ok().and_then(|mut guard| guard.take());
        match request {
            Some(request) => finish(build_frame(png, NativeCaptureBackend::WkWebView, request)),
            None => finish(Err(NativeCaptureError::Platform(
                "WKWebView snapshot callback completed more than once".into(),
            ))),
        }
    });

    // SAFETY: `view` is a live WKWebView borrowed by the caller. WebKit copies
    // or retains the completion block for the asynchronous snapshot operation.
    unsafe {
        view.takeSnapshotWithConfiguration_completionHandler(
            None::<&WKSnapshotConfiguration>,
            &block,
        );
    }
}
