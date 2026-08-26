#![deny(unsafe_op_in_unsafe_fn)]

mod platform;

use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use localview_capture::CaptureTarget;
pub use localview_protocol::ViewportMeta;
use serde::{Deserialize, Serialize};
use tauri::webview::PlatformWebview;
use thiserror::Error;

pub const MAX_PNG_BYTES: usize = 24 * 1024 * 1024;
pub const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const PNG_MIN_IHDR_BYTES: usize = 24;

type CaptureCompletion =
    Box<dyn FnOnce(Result<CapturedFrame, NativeCaptureError>) + Send + 'static>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureRequest {
    pub target: CaptureTarget,
    pub viewport: ViewportMeta,
    pub route: String,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeCaptureBackend {
    WebView2,
    WkWebView,
    WebKitGtk,
}

impl fmt::Display for NativeCaptureBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WebView2 => "webview2",
            Self::WkWebView => "wk_web_view",
            Self::WebKitGtk => "web_kit_gtk",
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct CapturedFrame {
    pub png: Vec<u8>,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub backend: NativeCaptureBackend,
    pub viewport: ViewportMeta,
    pub route: String,
    pub revision: Option<String>,
    pub captured_at_unix_ms: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NativeCaptureError {
    #[error("capture target is not supported by the native adapter")]
    UnsupportedTarget,
    #[error("native capture is not supported on this platform")]
    UnsupportedPlatform,
    #[error("webview is not ready for capture")]
    NotReady,
    #[error("native capture timed out")]
    Timeout,
    #[error("native capture platform error: {0}")]
    Platform(String),
    #[error("native capture did not return a valid PNG")]
    InvalidImage,
    #[error("native capture frame too large: {bytes} > {limit}")]
    FrameTooLarge { bytes: usize, limit: usize },
}

pub fn capture_webview(
    webview: PlatformWebview,
    request: CaptureRequest,
    completion: impl FnOnce(Result<CapturedFrame, NativeCaptureError>) + Send + 'static,
) {
    if request.target != CaptureTarget::Viewport {
        completion(Err(NativeCaptureError::UnsupportedTarget));
        return;
    }

    platform::capture(webview, request, Box::new(completion));
}

#[cfg(all(target_os = "macos", feature = "gui-smoke"))]
#[doc(hidden)]
pub fn capture_wk_webview_for_gui_smoke(
    view: &objc2_web_kit::WKWebView,
    request: CaptureRequest,
    completion: impl FnOnce(Result<CapturedFrame, NativeCaptureError>) + Send + 'static,
) {
    if request.target != CaptureTarget::Viewport {
        completion(Err(NativeCaptureError::UnsupportedTarget));
        return;
    }

    platform::capture_wk_webview_for_gui_smoke(view, request, Box::new(completion));
}

#[cfg(all(windows, feature = "gui-smoke"))]
#[doc(hidden)]
pub fn capture_webview2_for_gui_smoke(
    webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    request: CaptureRequest,
    completion: impl FnOnce(Result<CapturedFrame, NativeCaptureError>) + Send + 'static,
) {
    if request.target != CaptureTarget::Viewport {
        completion(Err(NativeCaptureError::UnsupportedTarget));
        return;
    }

    platform::capture_webview2_for_gui_smoke(webview, request, Box::new(completion));
}

pub fn validate_frame_size(bytes: usize) -> Result<(), NativeCaptureError> {
    if bytes > MAX_PNG_BYTES {
        return Err(NativeCaptureError::FrameTooLarge {
            bytes,
            limit: MAX_PNG_BYTES,
        });
    }
    Ok(())
}

pub fn validate_png(bytes: &[u8]) -> Result<(), NativeCaptureError> {
    validate_frame_size(bytes.len())?;
    if bytes.len() < PNG_MIN_IHDR_BYTES
        || !bytes.starts_with(PNG_SIGNATURE)
        || &bytes[12..16] != b"IHDR"
    {
        return Err(NativeCaptureError::InvalidImage);
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("fixed PNG width slice"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("fixed PNG height slice"));
    if width == 0 || height == 0 {
        return Err(NativeCaptureError::InvalidImage);
    }
    Ok(())
}

pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), NativeCaptureError> {
    validate_png(bytes)?;
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("validated PNG width slice"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("validated PNG height slice"));
    Ok((width, height))
}

pub fn build_frame(
    png: Vec<u8>,
    backend: NativeCaptureBackend,
    request: CaptureRequest,
) -> Result<CapturedFrame, NativeCaptureError> {
    let (pixel_width, pixel_height) = png_dimensions(&png)?;
    Ok(CapturedFrame {
        png,
        pixel_width,
        pixel_height,
        backend,
        viewport: request.viewport,
        route: request.route,
        revision: request.revision,
        captured_at_unix_ms: captured_at_unix_ms(),
    })
}

pub fn captured_at_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
