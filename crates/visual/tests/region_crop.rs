use localview_protocol::Rect;
use localview_visual::{
    crop_png_css_rect, decode_png_rgba, encode_png_rgba, RgbaImage, VisualError,
};

fn source_png() -> Vec<u8> {
    let mut data = Vec::new();
    for index in 0_u8..16 {
        data.extend_from_slice(&[index, 0, 0, 255]);
    }
    encode_png_rgba(&RgbaImage {
        width: 4,
        height: 4,
        data,
    })
    .expect("encode deterministic source PNG")
}

#[test]
fn crops_css_region_to_matching_native_pixels() {
    let png = source_png();
    let cropped = crop_png_css_rect(
        &png,
        (4, 4),
        (4.0, 4.0),
        &Rect {
            x: 1.0,
            y: 1.0,
            width: 2.0,
            height: 2.0,
        },
    )
    .expect("crop valid CSS region");

    let image = decode_png_rgba(&cropped).expect("decode cropped PNG");
    assert_eq!((image.width, image.height), (2, 2));
    let reds: Vec<u8> = image.data.chunks_exact(4).map(|pixel| pixel[0]).collect();
    assert_eq!(reds, vec![5, 6, 9, 10]);
}

#[test]
fn rejects_dimension_mismatch_before_region_processing() {
    let error = crop_png_css_rect(
        &source_png(),
        (8, 8),
        (4.0, 4.0),
        &Rect {
            x: 1.0,
            y: 1.0,
            width: 2.0,
            height: 2.0,
        },
    )
    .expect_err("native metadata mismatch must fail closed");

    assert!(matches!(error, VisualError::DimensionMismatch));
}

#[test]
fn rejects_zero_or_fully_offscreen_region() {
    let zero = crop_png_css_rect(
        &source_png(),
        (4, 4),
        (4.0, 4.0),
        &Rect {
            x: 1.0,
            y: 1.0,
            width: 0.0,
            height: 2.0,
        },
    )
    .expect_err("zero-width region must fail closed");
    assert!(matches!(zero, VisualError::InvalidRegionGeometry));

    let offscreen = crop_png_css_rect(
        &source_png(),
        (4, 4),
        (4.0, 4.0),
        &Rect {
            x: 8.0,
            y: 8.0,
            width: 2.0,
            height: 2.0,
        },
    )
    .expect_err("fully offscreen region must fail closed");
    assert!(matches!(offscreen, VisualError::InvalidRegionGeometry));
}
