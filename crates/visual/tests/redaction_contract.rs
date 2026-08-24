use localview_protocol::Rect;
use localview_visual::{redact_css_rects, RgbaImage, VisualError};

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
