use localview_protocol::Rect;
use localview_visual::{
    decode_png_rgba, encode_png_rgba, redact_css_rects, redact_png_css_rects, RgbaImage,
    VisualError,
};

fn solid_rgba(width: u32, height: u32, rgba: [u8; 4]) -> RgbaImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..width * height {
        data.extend_from_slice(&rgba);
    }
    RgbaImage { width, height, data }
}

fn pixel(image: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * image.width + x) * 4) as usize;
    image.data[offset..offset + 4].try_into().unwrap()
}

#[test]
fn css_mask_rects_scale_to_native_pixels_without_touching_neighbors() {
    let mut image = solid_rgba(4, 4, [255, 255, 255, 255]);
    let rects = [Rect {
        x: 0.5,
        y: 0.5,
        width: 1.0,
        height: 1.0,
    }];

    let applied = redact_css_rects(&mut image, (2.0, 2.0), &rects).unwrap();

    assert_eq!(applied, 1);
    for y in 0..4 {
        for x in 0..4 {
            let expected = if (1..3).contains(&x) && (1..3).contains(&y) {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            };
            assert_eq!(pixel(&image, x, y), expected, "pixel ({x},{y})");
        }
    }
}

#[test]
fn css_mask_rects_are_clamped_and_invalid_viewports_fail_closed() {
    let mut image = solid_rgba(4, 4, [20, 30, 40, 255]);
    let rects = [Rect {
        x: -1.0,
        y: -1.0,
        width: 2.0,
        height: 2.0,
    }];

    let applied = redact_css_rects(&mut image, (2.0, 2.0), &rects).unwrap();
    assert_eq!(applied, 1);
    assert_eq!(pixel(&image, 0, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&image, 1, 1), [0, 0, 0, 255]);
    assert_eq!(pixel(&image, 2, 2), [20, 30, 40, 255]);

    let error = redact_css_rects(&mut image, (0.0, 2.0), &rects).unwrap_err();
    assert!(matches!(error, VisualError::InvalidViewport));
}

#[test]
fn malformed_mask_geometry_fails_closed_before_any_pixel_is_changed() {
    for rect in [
        Rect {
            x: f64::NAN,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: -1.0,
            height: 1.0,
        },
        Rect {
            x: 0.0,
            y: f64::INFINITY,
            width: 1.0,
            height: 1.0,
        },
    ] {
        let original = solid_rgba(4, 4, [11, 22, 33, 255]);
        let mut image = original.clone();
        let error = redact_css_rects(&mut image, (2.0, 2.0), &[rect]).unwrap_err();

        assert!(matches!(error, VisualError::InvalidMaskGeometry));
        assert_eq!(image.data, original.data);
    }
}

#[test]
fn png_redaction_round_trips_native_pixels_and_checks_frame_dimensions() {
    let source = solid_rgba(4, 4, [120, 130, 140, 255]);
    let png = encode_png_rgba(&source).unwrap();
    let masks = [Rect {
        x: 0.5,
        y: 0.5,
        width: 1.0,
        height: 1.0,
    }];

    let (redacted_png, applied) =
        redact_png_css_rects(&png, (4, 4), (2.0, 2.0), &masks).unwrap();
    let redacted = decode_png_rgba(&redacted_png).unwrap();

    assert_eq!(applied, 1);
    assert_eq!((redacted.width, redacted.height), (4, 4));
    assert_eq!(pixel(&redacted, 1, 1), [0, 0, 0, 255]);
    assert_eq!(pixel(&redacted, 2, 2), [0, 0, 0, 255]);
    assert_eq!(pixel(&redacted, 0, 0), [120, 130, 140, 255]);

    let error = redact_png_css_rects(&png, (5, 4), (2.0, 2.0), &masks).unwrap_err();
    assert!(matches!(error, VisualError::DimensionMismatch));
}
