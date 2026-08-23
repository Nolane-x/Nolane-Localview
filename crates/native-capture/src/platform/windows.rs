use std::sync::{Arc, Mutex};

use tauri::webview::PlatformWebview;
use webview2_com::{
    CapturePreviewCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_15, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    },
};
use windows::{
    core::Interface,
    Win32::UI::Shell::SHCreateMemStream,
};

use crate::{
    build_frame, CaptureCompletion, CaptureRequest, NativeCaptureBackend, NativeCaptureError,
    MAX_PNG_BYTES,
};

type CompletionState = Arc<Mutex<Option<(CaptureCompletion, CaptureRequest)>>>;

fn finish(state: &CompletionState, result: Result<crate::CapturedFrame, NativeCaptureError>) {
    if let Ok(mut guard) = state.lock() {
        if let Some((completion, _)) = guard.take() {
            completion(result);
        }
    }
}

pub(crate) fn capture(
    webview: PlatformWebview,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    let state: CompletionState = Arc::new(Mutex::new(Some((completion, request))));

    // SAFETY: `SHCreateMemStream` returns a COM `IStream` owned by the returned
    // interface. The stream is only used on the WebView2 callback path and all
    // reads are bounded by `MAX_PNG_BYTES` before extending the Rust buffer.
    let stream = unsafe { SHCreateMemStream(None) };
    let Some(write_stream) = stream else {
        finish(
            &state,
            Err(NativeCaptureError::Platform(
                "unable to allocate WebView2 capture stream".into(),
            )),
        );
        return;
    };

    // SAFETY: Cloning an `IStream` creates an independent seek pointer over the
    // same backing bytes. The read clone remains at offset zero while WebView2
    // writes PNG bytes through `write_stream`.
    let read_stream = match unsafe { write_stream.Clone() } {
        Ok(stream) => stream,
        Err(error) => {
            finish(
                &state,
                Err(NativeCaptureError::Platform(error.to_string())),
            );
            return;
        }
    };

    let callback_state = Arc::clone(&state);
    let handler = CapturePreviewCompletedHandler::create(Box::new(move |capture_result| {
        if let Err(error) = capture_result {
            finish(
                &callback_state,
                Err(NativeCaptureError::Platform(error.to_string())),
            );
            return Ok(());
        }

        let mut png = Vec::new();
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let mut bytes_read = 0_u32;
            // SAFETY: `buffer` is writable for exactly `buffer.len()` bytes and
            // `bytes_read` is a valid out pointer for the duration of the COM call.
            let read_result = unsafe {
                read_stream.Read(
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    Some(&mut bytes_read),
                )
            };
            if let Err(error) = read_result {
                finish(
                    &callback_state,
                    Err(NativeCaptureError::Platform(error.to_string())),
                );
                return Ok(());
            }
            if bytes_read == 0 {
                break;
            }

            let next_len = png.len().saturating_add(bytes_read as usize);
            if next_len > MAX_PNG_BYTES {
                finish(
                    &callback_state,
                    Err(NativeCaptureError::FrameTooLarge {
                        bytes: next_len,
                        limit: MAX_PNG_BYTES,
                    }),
                );
                return Ok(());
            }
            png.extend_from_slice(&buffer[..bytes_read as usize]);
        }

        if let Ok(mut guard) = callback_state.lock() {
            if let Some((completion, request)) = guard.take() {
                completion(build_frame(png, NativeCaptureBackend::WebView2, request));
            }
        }
        Ok(())
    }));

    // SAFETY: The Tauri platform handle owns a live WebView2 controller on the
    // WebView main thread. The COM interfaces and handler remain strongly owned
    // through this call and WebView2 retains the completion handler as required.
    let start_result = unsafe {
        webview
            .controller()
            .CoreWebView2()
            .and_then(|core| core.cast::<ICoreWebView2_15>())
            .and_then(|core| {
                core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &write_stream,
                    &handler,
                )
            })
    };

    if let Err(error) = start_result {
        finish(
            &state,
            Err(NativeCaptureError::Platform(error.to_string())),
        );
    }
}
