use localview_protocol::Rect;
use localview_visual::{
    crop_png_css_rect, crop_rgba_css_rect, decode_png_rgba, encode_png_rgba, RgbaImage,
    VisualError,
};

fn source_image() -> RgbaImage {
    let mut data = Vec::new();
    for index in 0_u8..16 {
        data.extend_from_slice(&[index, 0, 0, 255]);
    }
    RgbaImage {
        width: 4,
        height: 4,
        data,
    }
}

fn source_png() -> Vec<u8> {
    encode_png_rgba(&source_image()).expect("encode deterministic source PNG")
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
fn crops_many_regions_directly_from_one_decoded_rgba_frame() {
    let image = source_image();
    let top_left = crop_rgba_css_rect(
        &image,
        (4.0, 4.0),
        &Rect {
            x: 0.0,
            y: 0.0,
            width: 2.0,
            height: 2.0,
        },
    )
    .expect("crop first region from decoded frame");
    let bottom_right = crop_rgba_css_rect(
        &image,
        (4.0, 4.0),
        &Rect {
            x: 2.0,
            y: 2.0,
            width: 2.0,
            height: 2.0,
        },
    )
    .expect("crop second region from same decoded frame");

    let first_reds: Vec<u8> = top_left
        .data
        .chunks_exact(4)
        .map(|pixel| pixel[0])
        .collect();
    let second_reds: Vec<u8> = bottom_right
        .data
        .chunks_exact(4)
        .map(|pixel| pixel[0])
        .collect();
    assert_eq!(first_reds, vec![0, 1, 4, 5]);
    assert_eq!(second_reds, vec![10, 11, 14, 15]);
    assert_eq!((image.width, image.height), (4, 4));
}

#[test]
fn direct_rgba_crop_rejects_invalid_geometry_without_mutating_source() {
    let image = source_image();
    let original = image.data.clone();
    let error = crop_rgba_css_rect(
        &image,
        (4.0, 4.0),
        &Rect {
            x: 8.0,
            y: 8.0,
            width: 2.0,
            height: 2.0,
        },
    )
    .expect_err("fully offscreen region must fail closed");

    assert!(matches!(error, VisualError::InvalidRegionGeometry));
    assert_eq!(image.data, original);
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