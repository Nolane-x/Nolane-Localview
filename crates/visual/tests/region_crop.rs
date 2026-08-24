use localview_protocol::Rect;
use localview_visual::{crop_png_css_rect, decode_png_rgba, encode_png_rgba, RgbaImage};

#[test]
fn crops_css_region_to_matching_native_pixels() {
    let mut data = Vec::new();
    for index in 0_u8..16 {
        data.extend_from_slice(&[index, 0, 0, 255]);
    }
    let source = RgbaImage {
        width: 4,
        height: 4,
        data,
    };
    let png = encode_png_rgba(&source).expect("encode deterministic source PNG");

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
