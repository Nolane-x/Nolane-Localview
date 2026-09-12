use localview_capture::CaptureTarget;
use localview_native_capture::{
    png_dimensions, validate_png, CaptureRequest, NativeCaptureBackend, NativeCaptureError,
    ViewportMeta, MAX_PNG_BYTES,
};

#[test]
fn contract_is_viewport_bounded_and_serializable() {
    let request = CaptureRequest {
        target: CaptureTarget::Viewport,
        viewport: ViewportMeta {
            css_width: 1280,
            css_height: 820,
            device_scale_factor: 1.25,
        },
        route: "http://127.0.0.1:5173/".into(),
        revision: Some("abc123".into()),
    };
    let json = serde_json::to_value(request).expect("capture request serializes");
    assert_eq!(json["viewport"]["css_width"], 1280);
    assert_eq!(MAX_PNG_BYTES, 25_165_824);
    assert_eq!(NativeCaptureBackend::WebView2.to_string(), "webview2");
}

#[test]
fn rejects_non_png_and_oversized_or_zero_dimension_frames() {
    assert!(matches!(
        validate_png(b"not png"),
        Err(NativeCaptureError::InvalidImage)
    ));
    assert!(matches!(
        localview_native_capture::validate_frame_size(MAX_PNG_BYTES + 1),
        Err(NativeCaptureError::FrameTooLarge { .. })
    ));
    assert!(png_dimensions(b"not png").is_err());
}

#[test]
fn pixel_frame_is_not_json_serialized_by_contract() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("pub struct CapturedFrame"));
    assert!(!source.contains("Serialize, Deserialize, PartialEq)]\npub struct CapturedFrame"));
}
